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

mod common;
mod kvm;

pub use common::Hypervisor;
use common::UniqueID;
use core::{fmt, result};
use kvm::KvmHypervisor;
use log::info;
use smccc::hvc64;

/// MMIO guard error.
#[derive(Debug, Clone)]
pub enum Error {
    // Unknown Hypervisor
    UnknownHypervisorUUID(u128),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::UnknownHypervisorUUID(e) => write!(f, "Unrecognized hypervisor UUID {:#x}", e),
        }
    }
}

/// Result type with mmio_guard::Error.
pub type Result<T> = result::Result<T, Error>;

static mut HYPERVISOR: HypervisorBackend = HypervisorBackend::Kvm(KvmHypervisor);

enum HypervisorBackend {
    Kvm(KvmHypervisor),
}

const ARM_SMCCC_VENDOR_HYP_CALL_UID_FUNC_ID: u32 = 0x8600ff01;

fn query_vendor_hyp_call_uid() -> u128 {
    let args = [0u64; 17];
    let res = hvc64(ARM_SMCCC_VENDOR_HYP_CALL_UID_FUNC_ID, args);
    (res[0] as u32 as u128)
        | ((res[1] as u32 as u128) << 32)
        | ((res[2] as u32 as u128) << 64)
        | ((res[3] as u32 as u128) << 96)
}

/// Gets the hypervisor singleton.
pub fn get_hypervisor() -> &'static dyn Hypervisor {
    // SAFETY - this is mutated only once as part of the initialization
    // and access to this happens only after initialization
    unsafe {
        match &HYPERVISOR {
            HypervisorBackend::Kvm(h) => h,
        }
    }
}

fn set_hypervisor(hyp: HypervisorBackend) -> Result<()> {
    // SAFETY - this is mutated only once as part of the initialization
    unsafe {
        HYPERVISOR = hyp;
    }
    info!("Detected hypervisor {}", get_hypervisor().name());
    Ok(())
}

/// Detect the hypervisor we are running on
pub fn detect_hypervisor() -> Result<()> {
    match query_vendor_hyp_call_uid() {
        KvmHypervisor::UUID => set_hypervisor(HypervisorBackend::Kvm(KvmHypervisor)),
        u => Err(Error::UnknownHypervisorUUID(u)),
    }
}
