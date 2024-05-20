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
use base::sys::linux::ioctl_with_val;
use binder::{self, Interface, IntoBinderResult, ParcelFileDescriptor};
use log::info;
use net_sys::TUNSETPERSIST;
use net_util::sys::linux::Tap;
use net_util::TapTCommon;
use std::ffi::CString;

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
        let ifname = CString::new(format!("avf_tap_{iface_name_suffix}"))
            .context(
                "Failed to construct TAP interface name as CString: avf_tap_{iface_name_suffix}",
            )
            .or_service_specific_exception(-1)?;
        let tap = Tap::new_with_name(ifname.as_c_str().to_bytes_with_nul(), true, false)
            .context("Failed to create TAP interface: {ifname:#?}")
            .or_service_specific_exception(-1)?;
        // SAFETY: Executing ioctl modifies the state of kernel, not this process. Execution failure
        // is checked below.
        let ret = unsafe { ioctl_with_val(&tap, TUNSETPERSIST(), 1) };
        if ret < 0 {
            return Err(anyhow!(
                "Failed to ioctl TUNSETPERSIST for the TAP interface: {ifname:#?}"
            ))
            .or_service_specific_exception(-1)?;
        }
        tap.enable()
            .context("Failed to enable TAP interface: {ifname:#?}")
            .or_service_specific_exception(-1)?;
        info!("Created TAP interface: {:#?}", ifname);
        Ok(ParcelFileDescriptor::new(tap.tap_file))
    }
}
