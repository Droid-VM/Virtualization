// Copyright 2025, The Android Open Source Project
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

use super::{DeviceAssigningHypervisor, Hypervisor, MemSharingHypervisor};
use crate::{mem::SIZE_4KB, Error, Result};
use core::fmt::{self, Display, Formatter};

const KVM_HC_PKVM_OP: u32 = 20;
const PKVM_GHC_SHARE_MEM: u32 = KVM_HC_PKVM_OP + 1;
const PKVM_GHC_UNSHARE_MEM: u32 = KVM_HC_PKVM_OP + 2;

const KVM_ENOSYS: i64 = -1000;
const KVM_EINVAL: i64 = -22;

/// This CPUID returns the signature and can be used to determine if VM is running under pKVM, KVM
/// or not.
pub const KVM_CPUID_SIGNATURE: u32 = 0x40000000;

/// Error from a KVM HVC call.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum KvmError {
    /// The call is not supported by the implementation.
    NotSupported,
    /// One of the call parameters has a non-supported value.
    InvalidParameter,
    /// There was an unexpected return value.
    Unknown(i64),
}

impl From<i64> for KvmError {
    fn from(value: i64) -> Self {
        match value {
            KVM_ENOSYS => KvmError::NotSupported,
            KVM_EINVAL => KvmError::InvalidParameter,
            _ => KvmError::Unknown(value),
        }
    }
}

impl From<i32> for KvmError {
    fn from(value: i32) -> Self {
        i64::from(value).into()
    }
}

impl Display for KvmError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::NotSupported => write!(f, "KVM call not supported"),
            Self::InvalidParameter => write!(f, "KVM call received non-supported value"),
            Self::Unknown(e) => write!(f, "Unknown return value from KVM {} ({0:#x})", e),
        }
    }
}

pub(super) struct RegularKvmHypervisor;

impl Hypervisor for RegularKvmHypervisor {}

pub(super) struct ProtectedKvmHypervisor;

impl Hypervisor for ProtectedKvmHypervisor {
    fn as_mem_sharer(&self) -> Option<&dyn MemSharingHypervisor> {
        Some(self)
    }

    fn as_device_assigner(&self) -> Option<&dyn DeviceAssigningHypervisor> {
        Some(self)
    }
}

impl MemSharingHypervisor for ProtectedKvmHypervisor {
    fn share(&self, base_ipa: u64) -> Result<()> {
        let ret: u32;
        // SAFETY:
        // Any undeclared register aren't clobbered except rbx but rbx value is restored at the end
        // of the asm block.
        unsafe {
            core::arch::asm!(
                "push rbx",
                "mov rbx, r8",
                "vmcall",
                "pop rbx",
                inout("rax") PKVM_GHC_SHARE_MEM => ret,
                in("r8") base_ipa,
                in("rcx") SIZE_4KB);
        };

        if ret != 0 {
            return Err(Error::KvmError(KvmError::from((ret as i32) as i64), 0));
        }

        Ok(())
    }

    fn unshare(&self, base_ipa: u64) -> Result<()> {
        let ret: u32;
        // SAFETY:
        // Any undeclared register aren't clobbered except rbx but rbx value is restored at the end
        // of the asm block.
        unsafe {
            core::arch::asm!(
                "push rbx",
                "mov rbx, r8",
                "vmcall",
                "pop rbx",
                inout("rax") PKVM_GHC_UNSHARE_MEM => ret,
                in("r8") base_ipa, in("rcx") SIZE_4KB);
        };

        if ret != 0 {
            return Err(Error::KvmError(KvmError::from((ret as i32) as i64), 0));
        }

        Ok(())
    }

    fn granule(&self) -> Result<usize> {
        Err(Error::KvmError(KvmError::NotSupported, 0))
    }
}

use crate::hypervisor::HypervisorBackend;

pub(crate) fn determine_hyp_type() -> Result<HypervisorBackend> {
    let s_kvm: u32;
    // SAFETY:
    // Any undeclared register aren't clobbered except rbx but rbx value is restored at the end of
    // the asm block.
    unsafe {
        // The argument for cpuid is passed via rax and in case of KVM_CPUID_SIGNATURE returned via
        // rbx, rcx and rdx. Ideally using named arguments in inline asm for rbx would be much
        // more straightforward but when rbx is directly used LLVM complains that:
        //      error: cannot use register `bx`: rbx is used internally by LLVM
        //      and cannot be used as an operand for inline asm
        //
        // Therefore use r8 instead and push rbx to the stack before making cpuid call, store
        // rbx content to r8 and use it as inline asm output, finally pop the rbx to restore
        // original value.
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov r8, rbx",
            "pop rbx",
            in("eax") KVM_CPUID_SIGNATURE, out("r8") s_kvm, out("rcx") _, out ("rdx") _);
    };

    // Check if running on pKVM or not
    if s_kvm.to_le_bytes() == "PKVM".as_bytes() {
        return Ok(HypervisorBackend::ProtectedKvm);
    }
    if s_kvm.to_le_bytes() == "KVMK".as_bytes() {
        return Ok(HypervisorBackend::RegularKvm);
    }

    Err(Error::UnsupportedHypervisorUuid(uuid::uuid!("deadbeef-8686-dead-beef-868686868688")))
}

impl DeviceAssigningHypervisor for ProtectedKvmHypervisor {
    fn get_phys_mmio_token(&self, _base_ipa: u64) -> Result<u64> {
        Err(Error::KvmError(KvmError::NotSupported, 0))
    }

    fn get_phys_iommu_token(&self, _pviommu_id: u64, _vsid: u64) -> Result<(u64, u64)> {
        Err(Error::KvmError(KvmError::NotSupported, 0))
    }
}
