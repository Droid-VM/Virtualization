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

//! Low-level allocation and tracking of main memory.

#![deny(unsafe_op_in_unsafe_fn)]

use crate::helpers::{self, align_down, align_up, page_4kb_of, SIZE_4KB};
use crate::hvc::{hyp_meminfo, mem_share, mem_unshare};
use crate::mmio_guard;
use crate::mmu;
use crate::smccc;
use aarch64_paging::paging::{Attributes, Descriptor, MemoryRegion as VaRange, PteUpdater};
use alloc::alloc::alloc_zeroed;
use alloc::alloc::dealloc;
use alloc::alloc::handle_alloc_error;
use core::alloc::Layout;
use core::arch::asm;
use core::cmp::max;
use core::cmp::min;
use core::fmt;
use core::num::NonZeroUsize;
use core::ops::Range;
use core::ptr::NonNull;
use core::result;
use log::{debug, error};
use spin::mutex::SpinMutex;
use tinyvec::ArrayVec;

pub type MemoryRange = Range<usize>;

pub static MEMORY: SpinMutex<Option<MemoryTracker>> = SpinMutex::new(None);
unsafe impl Send for MemoryTracker {}

const MMIO_LAZY_MAP_FLAG: Attributes = Attributes::SWFLAG_0;

#[derive(Clone, Copy, Debug, Default)]
enum MemoryType {
    #[default]
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Debug, Default)]
struct MemoryRegion {
    range: MemoryRange,
    mem_type: MemoryType,
}

impl MemoryRegion {
    /// True if the instance overlaps with the passed range.
    pub fn overlaps(&self, range: &MemoryRange) -> bool {
        overlaps(&self.range, range)
    }

    /// True if the instance is fully contained within the passed range.
    pub fn is_within(&self, range: &MemoryRange) -> bool {
        let our: &MemoryRange = self.as_ref();
        self.as_ref() == &(max(our.start, range.start)..min(our.end, range.end))
    }
}

impl AsRef<MemoryRange> for MemoryRegion {
    fn as_ref(&self) -> &MemoryRange {
        &self.range
    }
}

/// Returns true if one range overlaps with the other at all.
fn overlaps<T: Copy + Ord>(a: &Range<T>, b: &Range<T>) -> bool {
    max(a.start, b.start) < min(a.end, b.end)
}

/// Tracks non-overlapping slices of main memory.
pub struct MemoryTracker {
    total: MemoryRange,
    page_table: mmu::PageTable,
    regions: ArrayVec<[MemoryRegion; MemoryTracker::CAPACITY]>,
    mmio_regions: ArrayVec<[MemoryRange; MemoryTracker::MMIO_CAPACITY]>,
}

/// Errors for MemoryTracker operations.
#[derive(Debug, Clone)]
pub enum MemoryTrackerError {
    /// Tried to modify the memory base address.
    DifferentBaseAddress,
    /// Tried to shrink to a larger memory size.
    SizeTooLarge,
    /// Tracked regions would not fit in memory size.
    SizeTooSmall,
    /// Reached limit number of tracked regions.
    Full,
    /// Region is out of the tracked memory address space.
    OutOfRange,
    /// New region overlaps with tracked regions.
    Overlaps,
    /// Region couldn't be mapped.
    FailedToMap,
    /// Error from an MMIO guard call.
    MmioGuard(mmio_guard::Error),
    /// Error updating page table entry range.
    RangeUpdateFailed,
}

impl fmt::Display for MemoryTrackerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::DifferentBaseAddress => write!(f, "Received different base address"),
            Self::SizeTooLarge => write!(f, "Tried to shrink to a larger memory size"),
            Self::SizeTooSmall => write!(f, "Tracked regions would not fit in memory size"),
            Self::Full => write!(f, "Reached limit number of tracked regions"),
            Self::OutOfRange => write!(f, "Region is out of the tracked memory address space"),
            Self::Overlaps => write!(f, "New region overlaps with tracked regions"),
            Self::FailedToMap => write!(f, "Failed to map the new region"),
            Self::MmioGuard(e) => e.fmt(f),
            Self::RangeUpdateFailed => write!(f, "Failed to update page table range"),
        }
    }
}

impl From<mmio_guard::Error> for MemoryTrackerError {
    fn from(e: mmio_guard::Error) -> Self {
        Self::MmioGuard(e)
    }
}

type Result<T> = result::Result<T, MemoryTrackerError>;

impl MemoryTracker {
    const CAPACITY: usize = 5;
    const MMIO_CAPACITY: usize = 5;
    /// Base of the system's contiguous "main" memory.
    const BASE: usize = 0x8000_0000;
    /// First address that can't be translated by a level 1 TTBR0_EL1.
    const MAX_ADDR: usize = 1 << 39;
    // Hardware dirty bit management available flag (ID_AA64MMFR1_EL1.HAFDBS[1])
    const DBM_AVAILABLE: usize = 1 << 1;
    // TCR_EL1.{HA,HD} bits controlling hardware management of access and dirty state
    const TCR_EL1_HA_HD_BITS: usize = 3 << 39;

    /// Create a new instance from an active page table, covering the maximum RAM size.
    pub fn new(page_table: mmu::PageTable) -> Self {
        Self {
            total: Self::BASE..Self::MAX_ADDR,
            page_table,
            regions: ArrayVec::new(),
            mmio_regions: ArrayVec::new(),
        }
    }

    /// Resize the total RAM size.
    ///
    /// This function fails if it contains regions that are not included within the new size.
    pub fn shrink(&mut self, range: &MemoryRange) -> Result<()> {
        if range.start != self.total.start {
            return Err(MemoryTrackerError::DifferentBaseAddress);
        }
        if self.total.end < range.end {
            return Err(MemoryTrackerError::SizeTooLarge);
        }
        if !self.regions.iter().all(|r| r.is_within(range)) {
            return Err(MemoryTrackerError::SizeTooSmall);
        }

        self.total = range.clone();
        Ok(())
    }

    /// Allocate the address range for a const slice; returns None if failed.
    pub fn alloc_range(&mut self, range: &MemoryRange) -> Result<MemoryRange> {
        let region = MemoryRegion { range: range.clone(), mem_type: MemoryType::ReadOnly };
        self.check(&region)?;
        self.page_table.map_rodata(range).map_err(|e| {
            error!("Error during range allocation: {e}");
            MemoryTrackerError::FailedToMap
        })?;
        self.add(region)
    }

    /// Allocate the address range for a mutable slice; returns None if failed.
    pub fn alloc_range_mut(&mut self, range: &MemoryRange) -> Result<MemoryRange> {
        let region = MemoryRegion { range: range.clone(), mem_type: MemoryType::ReadWrite };
        self.check(&region)?;
        self.page_table.map_data_dbm(range).map_err(|e| {
            error!("Error during mutable range allocation: {e}");
            MemoryTrackerError::FailedToMap
        })?;
        self.add(region)
    }

    /// Allocate the address range for a const slice; returns None if failed.
    pub fn alloc(&mut self, base: usize, size: NonZeroUsize) -> Result<MemoryRange> {
        self.alloc_range(&(base..(base + size.get())))
    }

    /// Allocate the address range for a mutable slice; returns None if failed.
    pub fn alloc_mut(&mut self, base: usize, size: NonZeroUsize) -> Result<MemoryRange> {
        self.alloc_range_mut(&(base..(base + size.get())))
    }

    /// Checks that the given range of addresses is within the MMIO region, and then maps it
    /// appropriately.
    pub fn map_mmio_range(&mut self, range: MemoryRange) -> Result<()> {
        // MMIO space is below the main memory region.
        if range.end > self.total.start {
            return Err(MemoryTrackerError::OutOfRange);
        }
        if self.mmio_regions.iter().any(|r| overlaps(r, &range)) {
            return Err(MemoryTrackerError::Overlaps);
        }
        if self.mmio_regions.len() == self.mmio_regions.capacity() {
            return Err(MemoryTrackerError::Full);
        }

        self.page_table.map_device(&range).map_err(|e| {
            error!("Error during MMIO device mapping: {e}");
            MemoryTrackerError::FailedToMap
        })?;
        self.modify_range(&range, &flag_for_mmio_map)?;

        if self.mmio_regions.try_push(range).is_some() {
            return Err(MemoryTrackerError::Full);
        }

        Ok(())
    }

    /// Checks that the given region is within the range of the `MemoryTracker` and doesn't overlap
    /// with any other previously allocated regions, and that the regions ArrayVec has capacity to
    /// add it.
    fn check(&self, region: &MemoryRegion) -> Result<()> {
        if !region.is_within(&self.total) {
            return Err(MemoryTrackerError::OutOfRange);
        }
        if self.regions.iter().any(|r| r.overlaps(&region.range)) {
            return Err(MemoryTrackerError::Overlaps);
        }
        if self.regions.len() == self.regions.capacity() {
            return Err(MemoryTrackerError::Full);
        }
        Ok(())
    }

    fn add(&mut self, region: MemoryRegion) -> Result<MemoryRange> {
        if self.regions.try_push(region).is_some() {
            return Err(MemoryTrackerError::Full);
        }

        Ok(self.regions.last().unwrap().as_ref().clone())
    }

    /// Unmaps all tracked MMIO regions from the MMIO guard.
    ///
    /// Note that they are not unmapped from the page table.
    pub fn mmio_unmap_all(&self) -> Result<()> {
        for region in &self.mmio_regions {
            for page_base in page_iterator(region) {
                mmio_guard::unmap(page_base)?;
            }
        }

        Ok(())
    }

    /// Applies the updater function to PTEs corresponding to a virtual address range.
    pub fn modify_range(&mut self, range: &MemoryRange, f: &PteUpdater) -> Result<()> {
        self.page_table.modify_range(range, f).map_err(|e| {
            error!("Error updating page table entries: {e}");
            MemoryTrackerError::RangeUpdateFailed
        })
    }

    /// Sets up hardware dirty state management if available.
    pub fn setup_dbm() {
        let x: usize;
        // Safe because this only reads the value of memory model feature register.
        unsafe {
            asm!("mrs {}, id_aa64mmfr1_el1", out(reg) x);
        }
        if (x & Self::DBM_AVAILABLE) != 0 {
            // Safe because this sets the flags of TCR_EL1 to enable hardware
            // dirty state management.
            unsafe {
                asm!(
                    "mrs {tcr:x}, tcr_el1",
                    "orr {tcr:x}, {tcr:x}, {flags}",
                    "msr tcr_el1, {tcr:x}",
                    tcr = in(reg) 0usize,
                    flags = in(reg) Self::TCR_EL1_HA_HD_BITS,
                );
            }
        } else {
            debug!("hardware DBM not available");
        }
    }
}

impl Drop for MemoryTracker {
    fn drop(&mut self) {
        // Flush dirty pages.
        for i in 0..self.regions.len() {
            let region = &self.regions[i];
            match region.mem_type {
                MemoryType::ReadWrite => {
                    self.modify_range(
                        &Range { start: region.range.start, end: region.range.end },
                        &flush_dirty_range,
                    )
                    .unwrap();
                }
                MemoryType::ReadOnly => {}
            }
        }
    }
}

/// Gives the KVM host read, write and execute permissions on the given memory range. If the range
/// is not aligned with the memory protection granule then it will be extended on either end to
/// align.
fn share_range(range: &MemoryRange, granule: usize) -> smccc::Result<()> {
    for base in (align_down(range.start, granule)
        .expect("Memory protection granule was not a power of two")..range.end)
        .step_by(granule)
    {
        mem_share(base as u64)?;
    }
    Ok(())
}

/// Removes permission from the KVM host to access the given memory range which was previously
/// shared. If the range is not aligned with the memory protection granule then it will be extended
/// on either end to align.
fn unshare_range(range: &MemoryRange, granule: usize) -> smccc::Result<()> {
    for base in (align_down(range.start, granule)
        .expect("Memory protection granule was not a power of two")..range.end)
        .step_by(granule)
    {
        mem_unshare(base as u64)?;
    }
    Ok(())
}

/// Allocates a memory range of at least the given size from the global allocator, and shares it
/// with the host. Returns a pointer to the buffer.
///
/// It will be aligned to the memory sharing granule size supported by the hypervisor.
pub fn alloc_shared(size: usize) -> smccc::Result<NonNull<u8>> {
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
pub unsafe fn dealloc_shared(vaddr: NonNull<u8>, size: usize) -> smccc::Result<()> {
    let layout = shared_buffer_layout(size)?;
    let granule = layout.align();

    let paddr = virt_to_phys(vaddr);
    unshare_range(&(paddr..paddr + layout.size()), granule)?;
    // Safe because the memory was allocated by `alloc_shared` above using the same allocator, and
    // the layout is the same as was used then.
    unsafe { dealloc(vaddr.as_ptr(), layout) };

    Ok(())
}

/// Returns the layout to use for allocating a buffer of at least the given size shared with the
/// host.
///
/// It will be aligned to the memory sharing granule size supported by the hypervisor.
///
/// Panics if `size` is 0.
fn shared_buffer_layout(size: usize) -> smccc::Result<Layout> {
    assert_ne!(size, 0);
    let granule = hyp_meminfo()? as usize;
    let allocated_size =
        align_up(size, granule).expect("Memory protection granule was not a power of two");
    Ok(Layout::from_size_align(allocated_size, granule).unwrap())
}

/// Returns an iterator which yields the base address of each 4 KiB page within the given range.
fn page_iterator(range: &MemoryRange) -> impl Iterator<Item = usize> {
    (page_4kb_of(range.start)..range.end).step_by(SIZE_4KB)
}

/// Returns the intermediate physical address corresponding to the given virtual address.
///
/// As we use identity mapping for everything, this is just a cast, but it's useful to use it to be
/// explicit about where we are converting from virtual to physical address.
pub fn virt_to_phys(vaddr: NonNull<u8>) -> usize {
    vaddr.as_ptr() as _
}

/// Returns a pointer for the virtual address corresponding to the given non-zero intermediate
/// physical address.
///
/// Panics if `paddr` is 0.
pub fn phys_to_virt(paddr: usize) -> NonNull<u8> {
    NonNull::new(paddr as _).unwrap()
}

/// Checks whether a PTE at given level is a valid page or block desciptor.
fn is_block_or_page(desc: &Descriptor, level: usize) -> bool {
    const LEAF_PTE_LEVEL: usize = 3;
    level == LEAF_PTE_LEVEL && desc.is_table_or_page()
        || level < LEAF_PTE_LEVEL && !desc.is_table_or_page()
}

/// Flags a page table entry for later MMIO mapping.
fn flag_for_mmio_map(
    va_range: &VaRange,
    desc: &mut Descriptor,
    level: usize,
) -> core::result::Result<(), ()> {
    if is_block_or_page(desc, level) {
        desc.modify_flags(MMIO_LAZY_MAP_FLAG, Attributes::VALID);
        // Safe because it invalidates TLB and doesn't affect Rust.
        unsafe {
            asm!(
                "tlbi vae1is, {x}",
                "dsb ish",
                x = in(reg) 1usize << 48 & va_range.start().0 >> 12,
            );
        }
    }
    Ok(())
}

/// MMIO maps a page marked with MMIO_MAPPED flag and marks it as valid.
pub fn handle_flagged_page_fault(
    va_range: &VaRange,
    desc: &mut Descriptor,
    _level: usize,
) -> core::result::Result<(), ()> {
    if let Some(flags) = desc.flags() {
        if flags.contains(MMIO_LAZY_MAP_FLAG) {
            desc.modify_flags(Attributes::VALID, MMIO_LAZY_MAP_FLAG);
            for va in page_iterator(&(va_range.start().0..va_range.end().0)) {
                mmio_guard::map(va).map_err(|e| {
                    error!("{e}");
                })?;
            }
        }
        Ok(())
    } else {
        Err(())
    }
}

/// Flushes a memory range the descriptor refers to, if the descriptor is in writable-dirty state.
fn flush_dirty_range(
    va_range: &VaRange,
    desc: &mut Descriptor,
    level: usize,
) -> core::result::Result<(), ()> {
    // Only flush ranges corresponding to dirty leaf PTEs.
    if let Some(flags) = desc.flags() {
        if is_block_or_page(desc, level)
            && flags.contains(Attributes::DBM)
            && !flags.contains(Attributes::READ_ONLY)
        {
            helpers::flush_region(va_range.start().0, va_range.len());
        }
    }
    Ok(())
}

/// Clears read-only flag on a PTE, making it writable-dirty. Used when dirty state is managed
/// in software to handle permission faults on read-only descriptors.
pub fn handle_permission_fault(
    va_range: &VaRange,
    desc: &mut Descriptor,
    level: usize,
) -> core::result::Result<(), ()> {
    if let Some(flags) = desc.flags() {
        if is_block_or_page(desc, level) && flags.contains(Attributes::DBM) {
            desc.modify_flags(Attributes::empty(), Attributes::READ_ONLY);
            // Safe because it invalidates TLB and doesn't affect Rust.
            unsafe {
                asm!(
                    "tlbi vae1is, {x}",
                    "dsb ish",
                    x = in(reg) 1usize << 48 & va_range.start().0 >> 12,
                );
            }
        }
        Ok(())
    } else {
        Err(())
    }
}
