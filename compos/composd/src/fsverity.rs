/*
 * Copyright (C) 2023 The Android Open Source Project
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

use nix::ioctl_write_ptr;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::io::BorrowedFd;

// Constants/values from uapi/linux/fsverity.h
const FS_VERITY_HASH_ALG_SHA256: u32 = 1;
const FS_IOCTL_MAGIC: u8 = b'f';
const FS_IOC_ENABLE_VERITY: u8 = 133;

#[repr(C)]
pub struct fsverity_enable_arg {
    version: u32,
    hash_algorithm: u32,
    block_size: u32,
    salt_size: u32,
    salt_ptr: u64,
    sig_size: u32,
    __reserved1: u32,
    sig_ptr: u64,
    __reserved2: [u64; 11],
}

ioctl_write_ptr!(enable_verity, FS_IOCTL_MAGIC, FS_IOC_ENABLE_VERITY, fsverity_enable_arg);

/// Enable fs-verity to the `fd`, with sha256 hash algorithm and 4KB block size.
#[allow(dead_code)]
pub fn enable(fd: BorrowedFd) -> io::Result<()> {
    let arg = fsverity_enable_arg {
        version: 1,
        hash_algorithm: FS_VERITY_HASH_ALG_SHA256,
        block_size: 4096,
        salt_size: 0,
        salt_ptr: 0,
        sig_size: 0,
        __reserved1: 0,
        sig_ptr: 0,
        __reserved2: [0; 11],
    };
    if unsafe { enable_verity(fd.as_raw_fd(), &arg) } == Ok(0) {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
