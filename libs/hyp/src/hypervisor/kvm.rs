// Copyright 2022, The Android Open Source Project
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

//! Wrappers around calls to the KVM hypervisor.

use super::common::Hypervisor;
use core::fmt::{self, Display, Formatter};
use psci::smccc::{
    error::{positive_or_error_64, success_or_error_32, success_or_error_64},
    hvc64,
};

/// Error from a KVM HVC call.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Error {
    /// The call is not supported by the implementation.
    NotSupported,
    /// One of the call parameters has a non-supported value.
    InvalidParameter,
    /// There was an unexpected return value.
    Unknown(i64),
}

impl From<i64> for Error {
    fn from(value: i64) -> Self {
        match value {
            -1 => Error::NotSupported,
            -3 => Error::InvalidParameter,
            _ => Error::Unknown(value),
        }
    }
}

impl From<i32> for Error {
    fn from(value: i32) -> Self {
        i64::from(value).into()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::NotSupported => write!(f, "SMCCC call not supported"),
            Self::InvalidParameter => write!(f, "SMCCC call received non-supported value"),
            Self::Unknown(e) => write!(f, "Unknown SMCCC return value {} ({0:#x})", e),
        }
    }
}

const ARM_SMCCC_KVM_FUNC_HYP_MEMINFO: u32 = 0xc6000002;
const ARM_SMCCC_KVM_FUNC_MEM_SHARE: u32 = 0xc6000003;
const ARM_SMCCC_KVM_FUNC_MEM_UNSHARE: u32 = 0xc6000004;

const VENDOR_HYP_KVM_MMIO_GUARD_INFO_FUNC_ID: u32 = 0xc6000005;
const VENDOR_HYP_KVM_MMIO_GUARD_ENROLL_FUNC_ID: u32 = 0xc6000006;
const VENDOR_HYP_KVM_MMIO_GUARD_MAP_FUNC_ID: u32 = 0xc6000007;
const VENDOR_HYP_KVM_MMIO_GUARD_UNMAP_FUNC_ID: u32 = 0xc6000008;

pub(super) struct KvmHypervisor;

impl Hypervisor for KvmHypervisor {
    fn mmio_guard_granule(&self) -> Result<usize, Error> {
        let args = [0u64; 17];

        let granule =
            positive_or_error_64::<Error>(hvc64(VENDOR_HYP_KVM_MMIO_GUARD_INFO_FUNC_ID, args)[0])?;
        Ok(granule.try_into().unwrap())
    }

    fn mmio_guard_enroll(&self) -> Result<(), Error> {
        let args = [0u64; 17];

        success_or_error_64(hvc64(VENDOR_HYP_KVM_MMIO_GUARD_ENROLL_FUNC_ID, args)[0])
    }

    fn mmio_guard_map(&self, ipa: u64) -> Result<(), Error> {
        let mut args = [0u64; 17];
        args[0] = ipa;

        // TODO(b/277859415): pKVM returns a i32 instead of a i64 in T.
        // Drop this hack once T reaches EoL.
        success_or_error_32(hvc64(VENDOR_HYP_KVM_MMIO_GUARD_MAP_FUNC_ID, args)[0] as u32)
    }

    fn mmio_guard_unmap(&self, ipa: u64) -> Result<(), Error> {
        let mut args = [0u64; 17];
        args[0] = ipa;

        // TODO(b/277860860): pKVM returns NOT_SUPPORTED for SUCCESS in T.
        // Drop this hack once T reaches EoL.
        match success_or_error_64(hvc64(VENDOR_HYP_KVM_MMIO_GUARD_UNMAP_FUNC_ID, args)[0]) {
            Err(Error::NotSupported) | Ok(_) => Ok(()),
            x => x,
        }
    }

    fn mem_share(&self, base_ipa: u64) -> Result<(), Error> {
        let mut args = [0u64; 17];
        args[0] = base_ipa;

        success_or_error_64(hvc64(ARM_SMCCC_KVM_FUNC_MEM_SHARE, args)[0])
    }

    fn mem_unshare(&self, base_ipa: u64) -> Result<(), Error> {
        let mut args = [0u64; 17];
        args[0] = base_ipa;

        success_or_error_64(hvc64(ARM_SMCCC_KVM_FUNC_MEM_UNSHARE, args)[0])
    }

    fn memory_protection_granule(&self) -> Result<usize, Error> {
        let args = [0u64; 17];
        let granule =
            positive_or_error_64::<Error>(hvc64(ARM_SMCCC_KVM_FUNC_HYP_MEMINFO, args)[0])?;
        Ok(granule.try_into().unwrap())
    }
}
