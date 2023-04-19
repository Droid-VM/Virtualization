// Copyright 2023, The Android Open Source Project
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

//! Wrappers around hypervisor back-ends.

extern crate alloc;

mod common;
mod kvm;

use crate::error::{Error, Result};
use alloc::boxed::Box;
pub use common::Hypervisor;
pub use kvm::KvmError;
use kvm::KvmHypervisor;
use log::debug;
use once_cell::race::OnceBox;
use psci::smccc::hvc64;
use uuid::Uuid;

enum HypervisorBackend {
    Kvm,
}

impl HypervisorBackend {
    fn get_hypervisor(&self) -> &'static dyn Hypervisor {
        match self {
            Self::Kvm => &KvmHypervisor,
        }
    }
}

impl TryFrom<Uuid> for HypervisorBackend {
    type Error = Error;

    fn try_from(uuid: Uuid) -> Result<HypervisorBackend> {
        match uuid {
            KvmHypervisor::UUID => {
                debug!("Detected KVM Hypervisor");
                Ok(HypervisorBackend::Kvm)
            }
            u => {
                debug!("Unknown hypervisor UUID {}", u.urn());
                Err(Error::UnsupportedHypervisorUuid(u))
            }
        }
    }
}

const ARM_SMCCC_VENDOR_HYP_CALL_UID_FUNC_ID: u32 = 0x8600ff01;

fn query_vendor_hyp_call_uid() -> Uuid {
    let args = [0u64; 17];
    let res = hvc64(ARM_SMCCC_VENDOR_HYP_CALL_UID_FUNC_ID, args);
    Uuid::from_u128(
        (res[0] as u32 as u128)
            | ((res[1] as u32 as u128) << 32)
            | ((res[2] as u32 as u128) << 64)
            | ((res[3] as u32 as u128) << 96),
    )
}

fn detect_hypervisor() -> HypervisorBackend {
    query_vendor_hyp_call_uid().try_into().expect("Unknown hypervisor")
}

/// Gets the hypervisor singleton.
pub fn get_hypervisor() -> &'static dyn Hypervisor {
    static HYPERVISOR: OnceBox<HypervisorBackend> = OnceBox::new();

    HYPERVISOR.get_or_init(|| Box::new(detect_hypervisor())).get_hypervisor()
}
