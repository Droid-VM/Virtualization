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
pub use common::hypervisor_cap;
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
    let mut uid = [0u32; 4];
    let res = hvc64(ARM_SMCCC_VENDOR_HYP_CALL_UID_FUNC_ID, args);

    // Taking KVM's UUID as reference, "28b46fb6-2ec5-11e9-a9ca-4b564d003a74",
    // Uuid::from_u128() expects the input u128 value to be in the same order
    // i.e 0x28b46fb6_2ec511e9_a9ca4b56_4d003a74. ARM's SMC calling convention
    // (Document number ARM DEN 0028E) describes the UUID register mapping such
    // that W0 containts bytes 0..3 of UUID, with byte 0 in lower order bits. In
    // the KVM example, byte 0 (0x28) will be returned in the low 8-bits of W0,
    // while byte 15 (0x74) will be returned in upper 8 bits of W3.
    //
    // Do some byte swapping to present the 128-bit value in the order expected
    // by Uuid::from_u128() function.

    uid[0] = (res[0] as u32).swap_bytes();
    uid[1] = (res[1] as u32).swap_bytes();
    uid[2] = (res[2] as u32).swap_bytes();
    uid[3] = (res[3] as u32).swap_bytes();

    Uuid::from_u128(
        ((uid[0] as u128) << 96)
            | ((uid[1] as u128) << 64)
            | ((uid[2] as u128) << 32)
            | (uid[3] as u128),
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
