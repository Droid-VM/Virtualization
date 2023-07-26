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
mod geniezone;
mod gunyah;
mod kvm;

use crate::error::{Error, Result};
use alloc::boxed::Box;
use common::Hypervisor;
pub use common::{MemSharingHypervisor, MmioGuardedHypervisor, MMIO_GUARD_GRANULE_SIZE};
pub use geniezone::GeniezoneError;
use geniezone::GeniezoneHypervisor;
use gunyah::GunyahHypervisor;
pub use kvm::KvmError;
use kvm::{ProtectedKvmHypervisor, RegularKvmHypervisor};
use once_cell::race::OnceBox;
use smccc::{query_vendor_hyp_call_uid, Hvc};
use uuid::Uuid;

enum HypervisorBackend {
    RegularKvm,
    Gunyah,
    Geniezone,
    ProtectedKvm,
}

impl HypervisorBackend {
    fn get_hypervisor(&self) -> &'static dyn Hypervisor {
        match self {
            Self::RegularKvm => &RegularKvmHypervisor,
            Self::Gunyah => &GunyahHypervisor,
            Self::Geniezone => &GeniezoneHypervisor,
            Self::ProtectedKvm => &ProtectedKvmHypervisor,
        }
    }
}

impl TryFrom<Uuid> for HypervisorBackend {
    type Error = Error;

    fn try_from(uuid: Uuid) -> Result<HypervisorBackend> {
        match uuid {
            GeniezoneHypervisor::UUID => Ok(HypervisorBackend::Geniezone),
            GunyahHypervisor::UUID => Ok(HypervisorBackend::Gunyah),
            RegularKvmHypervisor::UUID => {
                // Protected KVM has the same UUID as "regular" KVM so issue an HVC that is assumed
                // to only be supported by pKVM: if it returns SUCCESS, deduce that this is pKVM
                // and if it returns NOT_SUPPORTED assume that it is "regular" KVM.
                match ProtectedKvmHypervisor.as_mmio_guard().unwrap().granule() {
                    Ok(_) => Ok(HypervisorBackend::ProtectedKvm),
                    Err(Error::KvmError(KvmError::NotSupported, _)) => {
                        Ok(HypervisorBackend::RegularKvm)
                    }
                    Err(e) => Err(e),
                }
            }
            u => Err(Error::UnsupportedHypervisorUuid(u)),
        }
    }
}

fn detect_hypervisor() -> HypervisorBackend {
    let uuid = query_vendor_hyp_call_uid::<Hvc>().unwrap();

    uuid.try_into().expect("Failed to detect hypervisor")
}

/// Gets the hypervisor singleton.
fn get_hypervisor() -> &'static dyn Hypervisor {
    static HYPERVISOR: OnceBox<HypervisorBackend> = OnceBox::new();

    HYPERVISOR.get_or_init(|| Box::new(detect_hypervisor())).get_hypervisor()
}

/// Gets the MMIO_GUARD hypervisor singleton, if any.
pub fn get_mmio_guard() -> Option<&'static dyn MmioGuardedHypervisor> {
    get_hypervisor().as_mmio_guard()
}

/// Gets the dynamic memory sharing hypervisor singleton, if any.
pub fn get_mem_sharer() -> Option<&'static dyn MemSharingHypervisor> {
    get_hypervisor().as_mem_sharer()
}
