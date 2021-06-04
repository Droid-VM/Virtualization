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

// `dm` module implements part of the `device-mapper` ioctl interfaces. It currently supports
// creation and deletion of the mapper device. It doesn't support other operations like querying
// the status of the mapper device. And there's no plan to extend the support unless it is
// required.
//
// Why in-house development? [`devicemapper`](https://crates.io/crates/devicemapper) is a public
// Rust implementation of the device mapper APIs. However, it doesn't provide any abstraction for
// the target-specific tables. User has to manually craft the table. Ironically, the library
// provides a lot of APIs for the features that are not required for `apkverity` such as listing
// the device mapper block devices that are currently listed in the kernel. Size is an important
// criteria for Microdroid.

use crate::util::*;

use anyhow::Result;
use bitflags::bitflags;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use uuid::Uuid;

mod verity;
pub use verity::*;

// UAPI for device mapper can be found at include/uapi/linux/dm-ioctl.h

const DM_IOCTL: u8 = 0xfd;

#[repr(u16)]
#[allow(dead_code)]
#[allow(non_camel_case_types)]
enum Cmd {
    DM_VERSION = 0,
    DM_REMOVE_ALL,
    DM_LIST_DEVICES,
    DM_DEV_CREATE,
    DM_DEV_REMOVE,
    DM_DEV_RENAME,
    DM_DEV_SUSPEND,
    DM_DEV_STATUS,
    DM_DEV_WAIT,
    DM_TABLE_LOAD,
    DM_TABLE_CLEAR,
    DM_TABLE_DEPS,
    DM_TABLE_STATUS,
    DM_LIST_VERSIONS,
    DM_TARGET_MSG,
    DM_DEV_SET_GEOMETRY,
}

nix::ioctl_readwrite!(dm_dev_create, DM_IOCTL, Cmd::DM_DEV_CREATE, DmIoctl);
nix::ioctl_readwrite!(dm_dev_remove, DM_IOCTL, Cmd::DM_DEV_REMOVE, DmIoctl);
nix::ioctl_readwrite!(dm_dev_suspend, DM_IOCTL, Cmd::DM_DEV_SUSPEND, DmIoctl);
nix::ioctl_readwrite!(dm_table_load, DM_IOCTL, Cmd::DM_TABLE_LOAD, DmIoctl);

#[repr(C)]
pub struct DmIoctl {
    version: [u32; 3],
    data_size: u32,
    data_start: u32,
    target_count: u32,
    open_count: i32,
    flags: Flag,
    event_nr: u32,
    padding: u32,
    dev: u64,
    name: [u8; DM_NAME_LEN],
    uuid: [u8; DM_UUID_LEN],
    data: [u8; 7],
}

const DM_VERSION_MAJOR: u32 = 4;
const DM_VERSION_MINOR: u32 = 0;
const DM_VERSION_PATCHLEVEL: u32 = 0;

const DM_NAME_LEN: usize = 128;
const DM_UUID_LEN: usize = 129;
const DM_MAX_TYPE_NAME: usize = 16;

bitflags! {
    struct Flag: u32 {
        const DM_READONLY_FLAG = 1 << 0;
        const DM_SUSPEND_FLAG = 1 << 1;
        const DM_PERSISTENT_DEV_FLAG = 1 << 3;
        const DM_STATUS_TABLE_FLAG = 1 << 4;
        const DM_ACTIVE_PRESENT_FLAG = 1 << 5;
        const DM_INACTIVE_PRESENT_FLAG = 1 << 6;
        const DM_BUFFER_FULL_FLAG = 1 << 8;
        const DM_SKIP_BDGET_FLAG = 1 << 9;
        const DM_SKIP_LOCKFS_FLAG = 1 << 10;
        const DM_NOFLUSH_FLAG = 1 << 11;
        const DM_QUERY_INACTIVE_TABLE_FLAG = 1 << 12;
        const DM_UEVENT_GENERATED_FLAG = 1 << 13;
        const DM_UUID_FLAG = 1 << 14;
        const DM_SECURE_DATA_FLAG = 1 << 15;
        const DM_DATA_OUT_FLAG = 1 << 16;
        const DM_DEFERRED_REMOVE = 1 << 17;
        const DM_INTERNAL_SUSPEND_FLAG = 1 << 18;
    }
}

// `DmTargetSpec` is the header of the data structure for a device-mapper target. When doing the
// ioctl, one of more `DmTargetSpec` (and its body) are appened to the `DmIoctl` struct.
#[repr(C)]
struct DmTargetSpec {
    sector_start: u64,
    length: u64, // number of 512 sectors
    status: i32,
    next: u32,
    target_type: [u8; DM_MAX_TYPE_NAME],
}

impl DmTargetSpec {
    fn new(target_type: &str) -> Result<Self> {
        let mut spec = unsafe { std::mem::MaybeUninit::<Self>::zeroed().assume_init() };
        spec.target_type.as_mut().write_all(target_type.as_bytes())?;
        Ok(spec)
    }

    fn as_u8_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts((self as *const Self) as *const u8, size_of::<Self>()) }
    }
}

impl DmIoctl {
    fn new(name: &str) -> Result<DmIoctl> {
        let mut data = unsafe { std::mem::MaybeUninit::<Self>::zeroed().assume_init() };
        data.version[0] = DM_VERSION_MAJOR;
        data.version[1] = DM_VERSION_MINOR;
        data.version[2] = DM_VERSION_PATCHLEVEL;
        data.data_size = size_of::<Self>() as u32;
        data.data_start = 0;
        data.name.as_mut().write_all(name.as_bytes())?;
        Ok(data)
    }

    fn set_uuid(&mut self, uuid: &str) -> Result<()> {
        let mut dst = self.uuid.as_mut();
        dst.fill(0);
        dst.write_all(uuid.as_bytes())?;
        Ok(())
    }

    fn as_u8_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts((self as *const Self) as *const u8, size_of::<Self>()) }
    }
}

/// `DeviceMapper` is the entry point for the device mapper framework. It essentially is a file
/// handle to "/dev/mapper/control".
pub struct DeviceMapper(File);

impl DeviceMapper {
    /// Constructs a new `DeviceMapper` entrypoint. This is essentially the same as opening
    /// "/dev/mapper/control".
    pub fn new() -> Result<DeviceMapper> {
        let f = OpenOptions::new().read(true).write(true).open("/dev/mapper/control").unwrap();
        Ok(DeviceMapper(f))
    }

    /// Creates a device mapper device and configure it according to the `target` specification.
    /// The path to the generated device is "/dev/mapper/<name>".
    pub fn create_device(&self, name: &str, target: &DmVerityTarget) -> Result<PathBuf> {
        let fd = self.0.as_raw_fd();

        // Step 1: create an empty device
        let mut data = DmIoctl::new(&name)?;
        data.set_uuid(&uuid())?;
        unsafe { dm_dev_create(fd, &mut data)? };

        // Step 2: load table onto the device
        let payload_size = size_of::<DmIoctl>() + target.as_u8_slice().len();

        let mut data = DmIoctl::new(&name)?;
        data.data_size = payload_size as u32;
        data.data_start = size_of::<DmIoctl>() as u32;
        data.target_count = 1;
        data.flags |= Flag::DM_READONLY_FLAG;

        let mut payload = Vec::with_capacity(payload_size);
        payload.extend_from_slice(&data.as_u8_slice());
        payload.extend_from_slice(&target.as_u8_slice());
        unsafe { dm_table_load(fd, payload.as_mut_ptr() as *mut DmIoctl)? };

        // Step 3: activate the device (note: the term 'suspend' might be misleading, but it
        // actually activates the table. See include/uapi/linux/dm-ioctl.h
        let mut data = DmIoctl::new(&name)?;
        unsafe { dm_dev_suspend(fd, &mut data)? };

        // Step 4: wait unti the device is created and return the device path
        let path = Path::new("/dev/mapper").join(&name);
        wait_for_path(&path)?;
        Ok(path)
    }

    /// Removes a mapper device
    pub fn delete_device_deferred(&self, name: &str) -> Result<()> {
        let mut data = DmIoctl::new(&name)?;
        data.flags |= Flag::DM_DEFERRED_REMOVE;
        unsafe { dm_dev_remove(self.0.as_raw_fd(), &mut data)? };
        Ok(())
    }
}

/// Used to derive a UUID that uniquely identifies a device mapper device when creating it.
// TODO(jiyong): the v4 is a randomly generated UUID. We might want another version of UUID (e.g.
// v3) where we can specify the namespace so that we can easily identify UUID's created for this
// purpose. For now, this random UUID is fine because we are expected to have only "one" instance
// of dm-verity device in Microdroid.
fn uuid() -> String {
    String::from(Uuid::new_v4().to_hyphenated().encode_lower(&mut Uuid::encode_buffer()))
}
