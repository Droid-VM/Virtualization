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

use crate::hypervisor::{Hypervisor, MemSharing, MmioGuard};
use crate::memory::shared_buffer_layout;
use crate::smccc;
use buddy_system_allocator::LockedHeap;
use core::ffi::CStr;
use core::ops::Range;
use core::ptr::NonNull;
use libfdt::Fdt;

pub struct GunyahHypervisor;

impl MmioGuard for GunyahHypervisor {}

impl MemSharing for GunyahHypervisor {
    fn mem_share(&self, _base_ipa: u64) -> smccc::Result<()> {
        Err(smccc::Error::NotSupported)
    }

    fn mem_unshare(&self, _base_ipa: u64) -> smccc::Result<()> {
        Err(smccc::Error::NotSupported)
    }

    fn alloc_shared(&self, size: usize) -> smccc::Result<NonNull<u8>> {
        let layout = shared_buffer_layout(size)?;

        Ok(SHM_ALLOCATOR.lock().alloc(layout).unwrap())
    }

    unsafe fn dealloc_shared(&self, vaddr: NonNull<u8>, size: usize) -> smccc::Result<()> {
        let layout = shared_buffer_layout(size)?;

        SHM_ALLOCATOR.lock().dealloc(vaddr, layout);

        Ok(())
    }
}

impl Hypervisor for GunyahHypervisor {}

static SHM_ALLOCATOR: LockedHeap<32> = LockedHeap::<32>::new();

// Return the node that has "restricted-dma-pool" compatible property set
fn rdma_node(fdt: &Fdt) -> libfdt::Result<libfdt::FdtNode> {
    fdt.compatible_nodes(CStr::from_bytes_with_nul(b"restricted-dma-pool\0").unwrap())?
        .next()
        .ok_or(libfdt::FdtError::NotFound)
}

// Return the range of memory indicated for swiotlb use
fn swiotlb_range(fdt: &libfdt::Fdt) -> libfdt::Result<Range<usize>> {
    let node = rdma_node(fdt)?;

    let reg =
        node.reg()?.ok_or(libfdt::FdtError::NotFound)?.next().ok_or(libfdt::FdtError::BadValue)?;

    let addr = reg.addr as usize;
    let size = reg.size.ok_or(libfdt::FdtError::BadValue)? as usize;

    if addr == 0 || size == 0 || addr.checked_add(size).is_none() {
        Err(libfdt::FdtError::BadValue)
    } else {
        Ok(addr..(addr + size))
    }
}

// Gunyah does not provide APIs for VM to share part of its memory with host OS.
// Instead APIs are available for host OS to "lend" part of its memory for VM's
// use and make it shared (both host OS and VM can access) or private (only VM
// can access). In case of protected VMs, host OS will lend some memory as
// "shared" and identify that range in a device-tree node that has
// "restricted-dma-pool" compatible property.
//
// This init() function will look for such a node if present and initialize the
// shared memory allocator to use the memory range described in the 'reg'
// property of that node.
pub fn init(fdt: &libfdt::Fdt) -> libfdt::Result<()> {
    let swiotlb = swiotlb_range(fdt);

    if let Ok(range) = swiotlb {
        // SAFETY - Assume that SHM_ALLOCATOR is initialized once with the
        // a range of memory that has been validated in `swiotlb_range`
        unsafe {
            SHM_ALLOCATOR.lock().init(range.start, range.end - range.start);
        }
        Ok(())
    } else {
        Err(swiotlb.expect_err("Expected values for swiotlb not found"))
    }
}
