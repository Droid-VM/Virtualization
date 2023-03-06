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

//! Wrappers around calls to the hypervisor.

use crate::smccc::{self, checked_hvc64, checked_hvc64_expect_zero};
use log::info;
use crate::hvc::Hypervisor;
use core::ptr::NonNull;
use crate::memory::shared_buffer_layout;
use alloc::alloc::alloc_zeroed;
use alloc::alloc::handle_alloc_error;
use crate::memory::virt_to_phys;
use crate::memory::share_range;
use crate::memory::unshare_range;
use alloc::alloc::dealloc;


const ARM_SMCCC_KVM_FUNC_HYP_MEMINFO: u32 = 0xc6000002;
const ARM_SMCCC_KVM_FUNC_MEM_SHARE: u32 = 0xc6000003;
const ARM_SMCCC_KVM_FUNC_MEM_UNSHARE: u32 = 0xc6000004;
const VENDOR_HYP_KVM_MMIO_GUARD_INFO_FUNC_ID: u32 = 0xc6000005;
const VENDOR_HYP_KVM_MMIO_GUARD_ENROLL_FUNC_ID: u32 = 0xc6000006;
const VENDOR_HYP_KVM_MMIO_GUARD_MAP_FUNC_ID: u32 = 0xc6000007;
const VENDOR_HYP_KVM_MMIO_GUARD_UNMAP_FUNC_ID: u32 = 0xc6000008;

const ARM_SMCCC_VENDOR_HYP_UID_KVM_REG_0: u64 = 0xb66fb428;
const ARM_SMCCC_VENDOR_HYP_UID_KVM_REG_1: u64 = 0xe911c52e;
const ARM_SMCCC_VENDOR_HYP_UID_KVM_REG_2: u64 = 0x564bcaa9;
const ARM_SMCCC_VENDOR_HYP_UID_KVM_REG_3: u64 = 0x743a004d;

pub fn is_kvm_hypervisor(args: [u64; 18], _fdt: &libfdt::Fdt) -> bool {
    args[0] == ARM_SMCCC_VENDOR_HYP_UID_KVM_REG_0 &&
       args[1] == ARM_SMCCC_VENDOR_HYP_UID_KVM_REG_1 &&
       args[2] == ARM_SMCCC_VENDOR_HYP_UID_KVM_REG_2 &&
       args[3] == ARM_SMCCC_VENDOR_HYP_UID_KVM_REG_3
}

pub struct KvmHypervisor;

impl Hypervisor for KvmHypervisor {

/// Queries the memory protection parameters for a protected virtual machine.
///
/// Returns the memory protection granule size in bytes.
fn hyp_meminfo(&self) -> smccc::Result<u64> {
    let args = [0u64; 17];
    checked_hvc64(ARM_SMCCC_KVM_FUNC_HYP_MEMINFO, args)
}

/// Shares a region of memory with the KVM host, granting it read, write and execute permissions.
/// The size of the region is equal to the memory protection granule returned by [`hyp_meminfo`].
fn mem_share(&self, base_ipa: u64) -> smccc::Result<()> {
    let mut args = [0u64; 17];
    args[0] = base_ipa;

    checked_hvc64_expect_zero(ARM_SMCCC_KVM_FUNC_MEM_SHARE, args)
}

/// Revokes access permission from the KVM host to a memory region previously shared with
/// [`mem_share`]. The size of the region is equal to the memory protection granule returned by
/// [`hyp_meminfo`].
fn mem_unshare(&self, base_ipa: u64) -> smccc::Result<()> {
    let mut args = [0u64; 17];
    args[0] = base_ipa;

    checked_hvc64_expect_zero(ARM_SMCCC_KVM_FUNC_MEM_UNSHARE, args)
}

fn mmio_guard_info(&self) -> smccc::Result<u64> {
    let args = [0u64; 17];

    checked_hvc64(VENDOR_HYP_KVM_MMIO_GUARD_INFO_FUNC_ID, args)
}

fn mmio_guard_enroll(&self) -> smccc::Result<()> {
    let args = [0u64; 17];

    checked_hvc64_expect_zero(VENDOR_HYP_KVM_MMIO_GUARD_ENROLL_FUNC_ID, args)
}

fn mmio_guard_map(&self, ipa: u64) -> smccc::Result<()> {
    let mut args = [0u64; 17];
    args[0] = ipa;

    // TODO(b/253586500): pKVM currently returns a i32 instead of a i64.
    let is_i32_error_code = |n| u32::try_from(n).ok().filter(|v| (*v as i32) < 0).is_some();
    match checked_hvc64_expect_zero(VENDOR_HYP_KVM_MMIO_GUARD_MAP_FUNC_ID, args) {
        Err(smccc::Error::Unexpected(e)) if is_i32_error_code(e) => {
            info!("Handled a pKVM bug by interpreting the MMIO_GUARD_MAP return value as i32");
            match e as u32 as i32 {
                -1 => Err(smccc::Error::NotSupported),
                -2 => Err(smccc::Error::NotRequired),
                -3 => Err(smccc::Error::InvalidParameter),
                ret => Err(smccc::Error::Unknown(ret as i64)),
            }
        }
        res => res,
    }
}

fn mmio_guard_unmap(&self, ipa: u64) -> smccc::Result<()> {
    let mut args = [0u64; 17];
    args[0] = ipa;

    // TODO(b/251426790): pKVM currently returns NOT_SUPPORTED for SUCCESS.
    match checked_hvc64_expect_zero(VENDOR_HYP_KVM_MMIO_GUARD_UNMAP_FUNC_ID, args) {
        Err(smccc::Error::NotSupported) | Ok(_) => Ok(()),
        x => x,
    }
}

/// Allocates a memory range of at least the given size from the global allocator, and shares it
/// with the host. Returns a pointer to the buffer.
///
/// It will be aligned to the memory sharing granule size supported by the hypervisor.
fn alloc_shared(&self, size: usize) -> smccc::Result<NonNull<u8>> {
    let layout = shared_buffer_layout(size)?;
    let granule = layout.align();

    // Safe because `shared_buffer_layout` panics if the size is 0, so the layout must have a
    // non-zero size.
    let buffer = unsafe { alloc_zeroed(layout) };

    let Some(buffer) = NonNull::new(buffer) else {
        handle_alloc_error(layout);
    };

    let paddr = virt_to_phys(buffer);
    // If share_range fails then we will leak the allocation, but that seems better than having it
    // be reused while maybe still partially shared with the host.
    share_range(&(paddr..paddr + layout.size()), granule)?;

    Ok(buffer)
}

/// Unshares and deallocates a memory range which was previously allocated by `alloc_shared`.
///
/// The size passed in must be the size passed to the original `alloc_shared` call.
///
/// # Safety
///
/// The memory must have been allocated by `alloc_shared` with the same size, and not yet
/// deallocated.
unsafe fn dealloc_shared(&self, vaddr: NonNull<u8>, size: usize) -> smccc::Result<()> {
    let layout = shared_buffer_layout(size)?;
    let granule = layout.align();

    let paddr = virt_to_phys(vaddr);
    unshare_range(&(paddr..paddr + layout.size()), granule)?;
    // Safe because the memory was allocated by `alloc_shared` above using the same allocator, and
    // the layout is the same as was used then.
    unsafe { dealloc(vaddr.as_ptr(), layout) };

    Ok(())
}

}
