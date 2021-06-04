/*
 * Copyright (C) 2021 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

// `loopdevice` module provides `attach` and `detach` functions that are for attaching and
// detaching a regular file to and from a loop device. Note that
// `loopdev`(https://crates.io/crates/loopdev) is a public alternative to this. In-house
// implementation was chosen to make Android-specific changes (like the use of the new
// LOOP_CONFIGURE instead of the legacy LOOP_SET_FD + LOOP_SET_STATUS64 combo which is considerably
// slower than the former).

use anyhow::{bail, Context, Result};
use bitflags::bitflags;
use std::fmt::Debug;
use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::util::*;

// This UAPI is copied and converted from include/uapi/linux/loop.h Note that this module doesn't
// implement all the features introduced in loop(4). Only the features that are required to support
// the `apkverity` use cases are implemented.

const LOOP_CONTROL: &str = "/dev/loop-control";

const LOOP_CTL_GET_FREE: libc::c_ulong = 0x4C82;
const LOOP_CONFIGURE: libc::c_ulong = 0x4C0A;
const LOOP_CLR_FD: libc::c_ulong = 0x4C01;

// These are old-style ioctls, thus *_bad.
nix::ioctl_none_bad!(loop_ctl_get_free, LOOP_CTL_GET_FREE);
nix::ioctl_write_ptr_bad!(loop_configure, LOOP_CONFIGURE, loop_config);
nix::ioctl_none_bad!(loop_clr_fd, LOOP_CLR_FD);

#[repr(C)]
pub struct loop_config {
    fd: u32,
    block_size: u32,
    info: loop_info64,
    reserved: [u64; 8],
}

#[repr(C)]
struct loop_info64 {
    lo_device: u64,
    lo_inode: u64,
    lo_rdevice: u64,
    lo_offset: u64,
    lo_sizelimit: u64,
    lo_number: u32,
    lo_encrypt_type: u32,
    lo_encrypt_key_size: u32,
    lo_flags: Flag,
    lo_file_name: [u8; LO_NAME_SIZE],
    lo_crypt_name: [u8; LO_NAME_SIZE],
    lo_encrypt_key: [u8; LO_KEY_SIZE],
    lo_init: [u64; 2],
}

bitflags! {
    struct Flag: u32 {
        const LO_FLAGS_READ_ONLY = 1 << 0;
        const LO_FLAGS_AUTOCLEAR = 1 << 2;
        const LO_FLAGS_PARTSCAN = 1 << 3;
        const LO_FLAGS_DIRECT_IO = 1 << 4;
    }
}

const LO_NAME_SIZE: usize = 64;
const LO_KEY_SIZE: usize = 32;

/// Creates a loop device and attach the given file at `path` as the backing store.
pub fn attach<P: AsRef<Path> + Debug>(path: P, offset: u64, size_limit: u64) -> Result<PathBuf> {
    // Attaching a file to a loop device can make a race condition; a loop device number obtained
    // from LOOP_CTL_GET_FREE might have been used by another thread or process. In that case the
    // subsequet LOOP_CONFIGURE ioctl returns with EBUSY. Try until it succeeds.
    //
    // Note that the timing parameters below are chosen rather arbitrarily. In practice (i.e.
    // inside Microdroid) we can't experience the race condition because `apkverity` is the only
    // user of /dev/loop-control at the moment. This loop is mostly for testing where multiple
    // tests run concurrently.
    const TIMEOUT: Duration = Duration::from_secs(1);
    const INTERVAL: Duration = Duration::from_millis(10);

    let begin = Instant::now();
    loop {
        match try_attach(&path, offset, size_limit) {
            Ok(loop_dev) => return Ok(loop_dev),
            _ => {
                if begin.elapsed() > TIMEOUT {
                    bail!("Can't attach {:?}", &path);
                }
            }
        };
        thread::sleep(INTERVAL);
    }
}

fn try_attach<P: AsRef<Path> + Debug>(path: P, offset: u64, size_limit: u64) -> Result<PathBuf> {
    // Get a free loop device
    wait_for_path(LOOP_CONTROL)?;
    let ctrl_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(LOOP_CONTROL)
        .context("Failed to open loop control")?;
    let num = unsafe {
        loop_ctl_get_free(ctrl_file.as_raw_fd()).context("Failed to get free loop device")?
    };

    // Construct the loop_config struct
    let backing_file =
        OpenOptions::new().read(true).open(&path).context(format!("failed to open {:?}", &path))?;
    let mut config = unsafe { std::mem::MaybeUninit::<loop_config>::zeroed().assume_init() };
    config.fd = backing_file.as_raw_fd() as u32;
    config.block_size = 4096;
    config.info.lo_offset = offset;
    config.info.lo_sizelimit = size_limit;
    config.info.lo_flags |= Flag::LO_FLAGS_DIRECT_IO | Flag::LO_FLAGS_READ_ONLY;

    // Configure the loop device to attach the backing file
    let device_path = format!("/dev/loop{}", num);
    wait_for_path(&device_path)?;
    let device_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&device_path)
        .context(format!("failed to open {:?}", &device_path))?;
    unsafe {
        loop_configure(device_file.as_raw_fd(), &mut config)
            .context(format!("Failed to configure {:?}", &device_path))?
    };

    Ok(PathBuf::from(device_path))
}

/// Detaches backing file from the loop device `path`.
pub fn detach<P: AsRef<Path>>(path: P) -> Result<()> {
    let device_file = OpenOptions::new().read(true).write(true).open(&path)?;
    unsafe { loop_clr_fd(device_file.as_raw_fd())? };
    Ok(())
}
