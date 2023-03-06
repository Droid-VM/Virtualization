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

#![allow(dead_code)]
#![allow(unused_variables)]

use crate::smccc;
use buddy_system_allocator::LockedHeap;
use libfdt::Fdt;
use core::ffi::CStr;
use core::ops::Range;
use crate::hvc::Hypervisor;
use crate::helpers::SIZE_4KB;
use core::ptr::NonNull;
use crate::memory::shared_buffer_layout;

// 0x19bd54bd, 0x0b37571b, 0x946f609b, 0x54539de6
//
// Gunyah (Qualcomm build)
const ARM_SMCCC_VENDOR_HYP_UID_QCOM_GUNYAH_REG_0: u64 = 0x19bd54bd;
const ARM_SMCCC_VENDOR_HYP_UID_QCOM_GUNYAH_REG_1: u64 = 0x0b37571b;
const ARM_SMCCC_VENDOR_HYP_UID_QCOM_GUNYAH_REG_2: u64 = 0x946f609b;
const ARM_SMCCC_VENDOR_HYP_UID_QCOM_GUNYAH_REG_3: u64 = 0x54539de6;

// Open Source Gunyah
// 0x673d5f14, 0x9265ce36, 0xa4535fdb, 0xc1d58fcd
const ARM_SMCCC_VENDOR_HYP_UID_GUNYAH_REG_0: u64 = 0x673d5f14;
const ARM_SMCCC_VENDOR_HYP_UID_GUNYAH_REG_1: u64 = 0x9265ce36;
const ARM_SMCCC_VENDOR_HYP_UID_GUNYAH_REG_2: u64 = 0xa4535fdb;
const ARM_SMCCC_VENDOR_HYP_UID_GUNYAH_REG_3: u64 = 0xc1d58fcd;

pub struct GunyahHypervisor;

static Shm_Allocator: LockedHeap<32> = LockedHeap::<32>::new();

fn rdma_node(fdt: &Fdt) -> libfdt::Result<libfdt::FdtNode> {
    fdt.compatible_nodes(CStr::from_bytes_with_nul(b"restricted-dma-pool\0").unwrap())?
        .next()
        .ok_or(libfdt::FdtError::NotFound)
}

fn swiotlb_range(fdt: &libfdt::Fdt) -> libfdt::Result<Range<usize>> {
    let node = rdma_node(fdt)?;

    let reg = node.reg()?
        .ok_or(libfdt::FdtError::NotFound)? // TODO: Unique Err values
        .next()
        .ok_or(libfdt::FdtError::BadValue)?;

    let addr = reg.addr as usize;
    let size = reg.size.ok_or(libfdt::FdtError::BadValue)? as usize;

    Ok(addr..(addr + size))
}


pub fn is_qcom_gunyah_hypervisor(args: [u64; 18], fdt: &libfdt::Fdt) -> bool {
    if args[0] != ARM_SMCCC_VENDOR_HYP_UID_QCOM_GUNYAH_REG_0 ||
       args[1] != ARM_SMCCC_VENDOR_HYP_UID_QCOM_GUNYAH_REG_1 ||
       args[2] != ARM_SMCCC_VENDOR_HYP_UID_QCOM_GUNYAH_REG_2 ||
       args[3] != ARM_SMCCC_VENDOR_HYP_UID_QCOM_GUNYAH_REG_3 {
            return false;
    }

    let swiotlb = swiotlb_range(fdt);

    if let Ok(range) = swiotlb {
        unsafe {
        Shm_Allocator.lock().init(range.start, range.end - range.start);
        }
        true
    } else {
        false
    }
}

impl Hypervisor for GunyahHypervisor {

/// Queries the memory protection parameters for a protected virtual machine.
///
/// Returns the memory protection granule size in bytes.
fn hyp_meminfo(&self) -> smccc::Result<u64> {
    Ok(SIZE_4KB as u64)
}

/// Shares a region of memory with the KVM host, granting it read, write and execute permissions.
/// The size of the region is equal to the memory protection granule returned by [`hyp_meminfo`].
fn mem_share(&self, base_ipa: u64) -> smccc::Result<()> {
    Err(smccc::Error::NotSupported)
}

/// Revokes access permission from the KVM host to a memory region previously shared with
/// [`mem_share`]. The size of the region is equal to the memory protection granule returned by
/// [`hyp_meminfo`].
fn mem_unshare(&self, base_ipa: u64) -> smccc::Result<()> {
    Err(smccc::Error::NotSupported)
}

fn mmio_guard_info(&self) -> smccc::Result<u64> {
    Ok(SIZE_4KB as u64)
}

fn mmio_guard_enroll(&self) -> smccc::Result<()> {
   Ok(())
}

fn mmio_guard_map(&self, ipa: u64) -> smccc::Result<()> {
   Ok(())
}

fn mmio_guard_unmap(&self, ipa: u64) -> smccc::Result<()> {
   Ok(())
}

/// Allocates a memory range of at least the given size from the global allocator, and shares it
/// with the host. Returns a pointer to the buffer.
///
/// It will be aligned to the memory sharing granule size supported by the hypervisor.
fn alloc_shared(&self, size: usize) -> smccc::Result<NonNull<u8>> {
    let layout = shared_buffer_layout(size)?;

    Ok(Shm_Allocator.lock().alloc(layout).unwrap())
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

    Shm_Allocator.lock().dealloc(vaddr, layout);

    Ok(())
}

}
