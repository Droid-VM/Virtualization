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

//! Wrappers around calls to the hypervisor.

#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(warnings)]

use log::error;
use crate::smccc;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::ptr::NonNull;
use crate::smccc::hvc64;
use crate::hvc::kvm_hypervisor::is_kvm_hypervisor;
use crate::hvc::gunyah_hypervisor::is_qcom_gunyah_hypervisor;
use crate::hvc::kvm_hypervisor::KvmHypervisor;
use crate::hvc::gunyah_hypervisor::GunyahHypervisor; // TODO: Use Gunyah everywhre
const ARM_SMCCC_VENDOR_HYP_CALL_UID_FUNC_ID: u32 = 0xc600ff01; 

const ARM_SMCCC_TRNG_VERSION: u32 = 0x8400_0050;
#[allow(dead_code)]
const ARM_SMCCC_TRNG_FEATURES: u32 = 0x8400_0051;
#[allow(dead_code)]
const ARM_SMCCC_TRNG_GET_UUID: u32 = 0x8400_0052;
#[allow(dead_code)]
const ARM_SMCCC_TRNG_RND32: u32 = 0x8400_0053;
const ARM_SMCCC_TRNG_RND64: u32 = 0xc400_0053;

pub mod trng;
pub mod kvm_hypervisor;
pub mod gunyah_hypervisor;

const UNINITIALIZED: usize = 0;
const INITIALIZING: usize = 1;
const INITIALIZED: usize = 2;

static HypInitState: AtomicUsize = AtomicUsize::new(0);

pub trait Hypervisor: Send {
    fn hyp_meminfo(&self) -> smccc::Result<u64>;
    fn mem_share(&self, base_ipa: u64) -> smccc::Result<()>;
    fn mem_unshare(&self, base_ipa: u64) -> smccc::Result<()>;
    fn mmio_guard_info(&self) -> smccc::Result<u64>;
    fn mmio_guard_enroll(&self) -> smccc::Result<()>;
    fn mmio_guard_map(&self, ipa: u64) -> smccc::Result<()>;
    fn mmio_guard_unmap(&self, ipa: u64) -> smccc::Result<()>;
    fn alloc_shared(&self, size: usize) -> smccc::Result<NonNull<u8>>;
    unsafe fn dealloc_shared(&self, vaddr: NonNull<u8>, size: usize) -> smccc::Result<()>;
}

struct UnknownHypervisor;

impl Hypervisor for UnknownHypervisor  {
    fn hyp_meminfo(&self) -> smccc::Result<u64> {
        Err(smccc::Error::NotSupported)
    }

    fn mem_share(&self, base_ipa: u64) -> smccc::Result<()> {
        Err(smccc::Error::NotSupported)
    }

    fn mem_unshare(&self, base_ipa: u64) -> smccc::Result<()> {
        Err(smccc::Error::NotSupported)
    }

    fn mmio_guard_info(&self) -> smccc::Result<u64> {
        Err(smccc::Error::NotSupported)
    }

    fn mmio_guard_enroll(&self) -> smccc::Result<()> {
        Err(smccc::Error::NotSupported)
    }

    fn mmio_guard_map(&self, ipa: u64) -> smccc::Result<()> {
        Err(smccc::Error::NotSupported)
    }

    fn mmio_guard_unmap(&self, ipa: u64) -> smccc::Result<()>  {
        Err(smccc::Error::NotSupported)
    }

    fn alloc_shared(&self, size: usize) -> smccc::Result<NonNull<u8>> {
        Err(smccc::Error::NotSupported)
    }

    unsafe fn dealloc_shared(&self, vaddr: NonNull<u8>, size: usize) -> smccc::Result<()> {
        Err(smccc::Error::NotSupported)
    }
}

static mut Cur_Hypervisor: &dyn Hypervisor = &UnknownHypervisor;

pub unsafe fn hypervisor_init(fdt: &libfdt::Fdt) -> Result<(), ()>
{
    let old_state = HypInitState.compare_exchange(UNINITIALIZED, INITIALIZING, Ordering::SeqCst, Ordering::SeqCst);

    match old_state {
            Ok(UNINITIALIZED) => {
                let args = [0u64; 17];
                let res = hvc64(ARM_SMCCC_VENDOR_HYP_CALL_UID_FUNC_ID, args);

                if is_kvm_hypervisor(res, fdt) {
                        Cur_Hypervisor = &KvmHypervisor;
                } else if is_qcom_gunyah_hypervisor(res, fdt) {
                        Cur_Hypervisor = &GunyahHypervisor;
                } else {
                    error!("Unknown Hypervisor!");
                    return Err(());
                }

                HypInitState.compare_exchange(INITIALIZING, INITIALIZED, Ordering::SeqCst, Ordering::SeqCst);

                Ok(())
            }

            _ =>  {
                error!("Unexpected state of HypInitState {:?} ", old_state);
                Err(())
            }
    }
}

/// Queries the memory protection parameters for a protected virtual machine.
///
/// Returns the memory protection granule size in bytes.
pub fn hyp_meminfo() -> smccc::Result<u64> {
        unsafe {Cur_Hypervisor.hyp_meminfo()}
}

/// Shares a region of memory with the KVM host, granting it read, write and execute permissions.
/// The size of the region is equal to the memory protection granule returned by [`hyp_meminfo`].
pub fn mem_share(base_ipa: u64) -> smccc::Result<()> {
        unsafe {
            Cur_Hypervisor.mem_share(base_ipa)
        }
}

/// Revokes access permission from the KVM host to a memory region previously shared with
/// [`mem_share`]. The size of the region is equal to the memory protection granule returned by
/// [`hyp_meminfo`].
pub fn mem_unshare(base_ipa: u64) -> smccc::Result<()> {
    unsafe {
        Cur_Hypervisor.mem_unshare(base_ipa)
    }
}

pub fn mmio_guard_info() -> smccc::Result<u64> {
    unsafe {
        Cur_Hypervisor.mmio_guard_info()
    }
}

pub fn mmio_guard_enroll() -> smccc::Result<()> {
    unsafe {
        Cur_Hypervisor.mmio_guard_enroll()
    }
}

pub fn mmio_guard_map(ipa: u64) -> smccc::Result<()> {
    unsafe {
        Cur_Hypervisor.mmio_guard_map(ipa)
    }
}

pub fn mmio_guard_unmap(ipa: u64) -> smccc::Result<()> {
    unsafe {
        Cur_Hypervisor.mmio_guard_unmap(ipa)
    }
}

pub fn alloc_shared(size: usize) -> smccc::Result<NonNull<u8>> {
    unsafe {
        Cur_Hypervisor.alloc_shared(size)
    }
}

pub unsafe fn dealloc_shared(vaddr: NonNull<u8>, size: usize) -> smccc::Result<()> {
    unsafe {
        Cur_Hypervisor.dealloc_shared(vaddr, size)
    }
}

/// Returns the (major, minor) version tuple, as defined by the SMCCC TRNG.
pub fn trng_version() -> trng::Result<(u16, u16)> {
    let args = [0u64; 17];

    let version = trng::hvc64(ARM_SMCCC_TRNG_VERSION, args)?[0];
    Ok(((version >> 16) as u16, version as u16))
}

pub type TrngRng64Entropy = (u64, u64, u64);

pub fn trng_rnd64(nbits: u64) -> trng::Result<TrngRng64Entropy> {
    let mut args = [0u64; 17];
    args[0] = nbits;

    let regs = trng::hvc64_expect_zero(ARM_SMCCC_TRNG_RND64, args)?;

    Ok((regs[1], regs[2], regs[3]))
}
