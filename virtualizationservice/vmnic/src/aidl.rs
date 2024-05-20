// Copyright 2024, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Implementation of the AIDL interface of Vmnic.

use anyhow::{anyhow, Context};
use android_system_virtualizationservice_internal::aidl::android::system::virtualizationservice_internal::IVmnic::IVmnic;
use binder::{self, ExceptionCode, Interface, IntoBinderResult, ParcelFileDescriptor};
use libc::{ifreq, IFF_NO_PI, IFF_TAP, IFF_UP, IFNAMSIZ};
use log::info;
use nix::ioctl_write_ptr_bad;
use std::fs::File;
use std::os::fd::AsRawFd;

const TUNSETIFF: i32 = 0x400454ca;

ioctl_write_ptr_bad!(create_tap_ioctl, TUNSETIFF, ifreq);

#[derive(Debug, Default)]
pub struct Vmnic {}

impl Vmnic {
    pub fn init() -> Vmnic {
        Vmnic::default()
    }
}

impl Interface for Vmnic {}

impl IVmnic for Vmnic {
    fn createTapInterface(&self, iface_name_suffix: &str) -> binder::Result<ParcelFileDescriptor> {
        let ifname = format!("avf_tap_{iface_name_suffix}");
        info!("Creating TAP interface {}", ifname);

        let tunfd = File::open("/dev/tun")
            .context("Failed to open /dev/tun")
            .or_service_specific_exception(-1)?;

        // SAFETY: Zero-filling the variable.
        let mut ifr: ifreq = unsafe { std::mem::zeroed() };

        ifr.ifr_ifru.ifru_flags = (IFF_TAP | IFF_NO_PI | IFF_UP) as i16;
        if ifname.len() + 1 > IFNAMSIZ {
            return Err(anyhow!(format!("TAP interface name {ifname} is too long")))
                .or_binder_exception(ExceptionCode::ILLEGAL_ARGUMENT);
        }
        ifr.ifr_name[..ifname.len()].copy_from_slice(ifname.as_bytes());

        // SAFETY: `ioctl` is copied into the kernel. It modifies the state in the kernel, not the
        // state of this process in any way.
        unsafe { create_tap_ioctl(tunfd.as_raw_fd(), &ifr) }
            .context("Failed to request ioctl for creating TAP network interface")
            .or_service_specific_exception(-1)?;

        Ok(ParcelFileDescriptor::new(tunfd))
    }
}
