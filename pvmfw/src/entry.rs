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

//! Low-level entry and exit points of pvmfw.

use crate::config;
use crate::fdt;
use crate::heap;
use crate::helpers;
use crate::mmio_guard;
use crate::mmu;
use core::arch::asm;
use core::cmp::max;
use core::cmp::min;
use core::fmt;
use core::mem;
use core::mem::MaybeUninit;
use core::num::NonZeroUsize;
use core::ops::Range;
use core::result;
use core::slice;
use log::debug;
use log::error;
use log::info;
use log::warn;
use log::LevelFilter;
use vmbase::{console, layout, logger, main, power::reboot};

#[derive(Debug, Clone)]
enum RebootReason {
    /// A malformed BCC was received.
    InvalidBcc,
    /// An invalid configuration was appended to pvmfw.
    InvalidConfig,
    /// An unexpected internal error happened.
    InternalError,
    /// The provided FDT was invalid.
    InvalidFdt,
    /// The provided payload was invalid.
    InvalidPayload,
    /// The provided ramdisk was invalid.
    InvalidRamdisk,
}

main!(start);

/// Entry point for pVM firmware.
pub fn start(fdt_address: u64, payload_start: u64, payload_size: u64, _arg3: u64) {
    // Limitations in this function:
    // - can't access non-pvmfw memory (only statically-mapped memory)
    // - can't access MMIO (therefore, no logging)

    match main_wrapper(fdt_address as usize, payload_start as usize, payload_size as usize) {
        Ok(_) => jump_to_payload(fdt_address, payload_start),
        Err(_) => reboot(),
    }

    // if we reach this point and return, vmbase::entry::rust_entry() will call power::shutdown().
}

type MemoryRange = Range<usize>;

#[derive(Clone, Copy, Debug)]
enum MemoryType {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Debug)]
struct MemoryRegion {
    range: MemoryRange,
    mem_type: MemoryType,
}

impl MemoryRegion {
    /// True if the instance overlaps with the passed range.
    pub fn overlaps(&self, range: &MemoryRange) -> bool {
        let our: &MemoryRange = self.as_ref();
        max(our.start, range.start) < min(our.end, range.end)
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

/// Tracks non-overlapping slices of main memory.
pub struct MemoryTracker<'a> {
    // TODO: Use tinyvec::ArrayVec
    count: usize,
    regions: [MaybeUninit<MemoryRegion>; MemoryTracker::CAPACITY],
    total: MemoryRange,
    page_table: &'a mut mmu::PageTable,
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
        }
    }
}

type MemoryTrackerResult<T> = result::Result<T, MemoryTrackerError>;

impl<'a> MemoryTracker<'a> {
    const CAPACITY: usize = 5;
    /// Base of the system's contiguous "main" memory.
    const BASE: usize = 0x8000_0000;
    /// First address that can't be translated by a level 1 TTBR0_EL1.
    const MAX_ADDR: usize = 1 << 39;

    /// Create a new instance from an active page table, covering the maximum RAM size.
    pub fn new(page_table: &'a mut mmu::PageTable) -> Self {
        Self {
            total: Self::BASE..Self::MAX_ADDR,
            count: 0,
            page_table,
            // SAFETY - MaybeUninit items (of regions) do not require initialization.
            regions: unsafe { MaybeUninit::uninit().assume_init() },
        }
    }

    /// Resize the total RAM size.
    ///
    /// This function fails if it contains regions that are not included within the new size.
    pub fn shrink(&mut self, range: &MemoryRange) -> MemoryTrackerResult<()> {
        if range.start != self.total.start {
            return Err(MemoryTrackerError::DifferentBaseAddress);
        }
        if self.total.end < range.end {
            return Err(MemoryTrackerError::SizeTooLarge);
        }
        if !self.regions().iter().all(|r| r.is_within(range)) {
            return Err(MemoryTrackerError::SizeTooSmall);
        }

        self.total = range.clone();
        Ok(())
    }

    /// Allocate the address range for a const slice; returns None if failed.
    pub fn alloc_range(&mut self, range: &MemoryRange) -> MemoryTrackerResult<MemoryRange> {
        self.page_table.map_rodata(range).map_err(|e| {
            error!("Error during range allocation: {e}");
            MemoryTrackerError::FailedToMap
        })?;
        self.add(MemoryRegion { range: range.clone(), mem_type: MemoryType::ReadOnly })
    }

    /// Allocate the address range for a mutable slice; returns None if failed.
    pub fn alloc_range_mut(&mut self, range: &MemoryRange) -> MemoryTrackerResult<MemoryRange> {
        self.page_table.map_data(range).map_err(|e| {
            error!("Error during mutable range allocation: {e}");
            MemoryTrackerError::FailedToMap
        })?;
        self.add(MemoryRegion { range: range.clone(), mem_type: MemoryType::ReadWrite })
    }

    /// Allocate the address range for a const slice; returns None if failed.
    pub fn alloc(&mut self, base: usize, size: NonZeroUsize) -> MemoryTrackerResult<MemoryRange> {
        self.alloc_range(&(base..(base + size.get())))
    }

    /// Allocate the address range for a mutable slice; returns None if failed.
    pub fn alloc_mut(
        &mut self,
        base: usize,
        size: NonZeroUsize,
    ) -> MemoryTrackerResult<MemoryRange> {
        self.alloc_range_mut(&(base..(base + size.get())))
    }

    fn regions(&self) -> &[MemoryRegion] {
        // SAFETY - The first self.count regions have been properly initialized.
        unsafe { mem::transmute::<_, &[MemoryRegion]>(&self.regions[..self.count]) }
    }

    fn add(&mut self, region: MemoryRegion) -> MemoryTrackerResult<MemoryRange> {
        if !region.is_within(&self.total) {
            return Err(MemoryTrackerError::OutOfRange);
        }
        if self.regions().iter().any(|r| r.overlaps(region.as_ref())) {
            return Err(MemoryTrackerError::Overlaps);
        }
        if self.regions.len() == self.count {
            return Err(MemoryTrackerError::Full);
        }

        let region = self.regions[self.count].write(region);
        self.count += 1;
        Ok(region.as_ref().clone())
    }
}

impl<'a> Drop for MemoryTracker<'a> {
    fn drop(&mut self) {
        for region in self.regions().iter() {
            match region.mem_type {
                MemoryType::ReadWrite => {
                    // TODO: Use page table's dirty bit to only flush pages that were touched.
                    helpers::flush_region(region.range.start, region.range.len())
                }
                MemoryType::ReadOnly => {}
            }
        }
    }
}

struct MemorySlices<'a> {
    fdt: &'a mut libfdt::Fdt,
    kernel: &'a [u8],
    ramdisk: Option<&'a [u8]>,
}

impl<'a> MemorySlices<'a> {
    fn new(
        fdt: usize,
        payload: usize,
        payload_size: usize,
        memory: &mut MemoryTracker,
    ) -> Result<Self, RebootReason> {
        // SAFETY - SIZE_2MB is non-zero.
        const FDT_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(helpers::SIZE_2MB) };
        // TODO - Only map the FDT as read-only, until we modify it right before jump_to_payload()
        // e.g. by generating a DTBO for a template DT in main() and, on return, re-map DT as RW,
        // overwrite with the template DT and apply the DTBO.
        let range = memory.alloc_mut(fdt, FDT_SIZE).map_err(|e| {
            error!("Failed to allocate the FDT range: {e}");
            RebootReason::InternalError
        })?;

        // SAFETY - The tracker validated the range to be in main memory, mapped, and not overlap.
        let fdt = unsafe { slice::from_raw_parts_mut(range.start as *mut u8, range.len()) };
        let fdt = libfdt::Fdt::from_mut_slice(fdt).map_err(|e| {
            error!("Failed to spawn the FDT wrapper: {e}");
            RebootReason::InvalidFdt
        })?;

        debug!("Fdt passed validation!");

        let memory_range = fdt
            .memory()
            .map_err(|e| {
                error!("Failed to get /memory from the DT: {e}");
                RebootReason::InvalidFdt
            })?
            .next()
            .ok_or_else(|| {
                error!("Failed to read the memory size from the FDT");
                RebootReason::InternalError
            })?;

        debug!("Resizing MemoryTracker to range {memory_range:#x?}");

        memory.shrink(&memory_range).map_err(|_| {
            error!("Failed to use memory range value from DT: {memory_range:#x?}");
            RebootReason::InvalidFdt
        })?;

        let payload_size = NonZeroUsize::new(payload_size).ok_or_else(|| {
            error!("Invalid payload size: {payload_size:#x}");
            RebootReason::InvalidPayload
        })?;

        let payload_range = memory.alloc(payload, payload_size).map_err(|e| {
            error!("Failed to obtain the payload range: {e}");
            RebootReason::InternalError
        })?;
        // SAFETY - The tracker validated the range to be in main memory, mapped, and not overlap.
        let kernel =
            unsafe { slice::from_raw_parts(payload_range.start as *const u8, payload_range.len()) };

        let ramdisk_range = fdt::initrd_range(fdt).map_err(|e| {
            error!("An error occurred while locating the ramdisk in the device tree: {e}");
            RebootReason::InternalError
        })?;

        let ramdisk = if let Some(r) = ramdisk_range {
            debug!("Located ramdisk at {r:?}");
            let r = memory.alloc_range(&r).map_err(|e| {
                error!("Failed to obtain the initrd range: {e}");
                RebootReason::InvalidRamdisk
            })?;

            // SAFETY - The region was validated by memory to be in main memory, mapped, and
            // not overlap.
            Some(unsafe { slice::from_raw_parts(r.start as *const u8, r.len()) })
        } else {
            info!("Couldn't locate the ramdisk from the device tree");
            None
        };

        Ok(Self { fdt, kernel, ramdisk })
    }
}

/// Sets up the environment for main() and wraps its result for start().
///
/// Provide the abstractions necessary for start() to abort the pVM boot and for main() to run with
/// the assumption that its environment has been properly configured.
fn main_wrapper(fdt: usize, payload: usize, payload_size: usize) -> Result<(), RebootReason> {
    // Limitations in this function:
    // - only access MMIO once (and while) it has been mapped and configured
    // - only perform logging once the logger has been initialized
    // - only access non-pvmfw memory once (and while) it has been mapped

    // SAFETY - This function should and will only be called once, here.
    unsafe { heap::init() };

    logger::init(LevelFilter::Info).map_err(|_| RebootReason::InternalError)?;

    // Use debug!() to avoid printing to the UART if we failed to configure it as only local
    // builds that have tweaked the logger::init() call will actually attempt to log the message.

    mmio_guard::init().map_err(|e| {
        debug!("{e}");
        RebootReason::InternalError
    })?;

    mmio_guard::map(console::BASE_ADDRESS).map_err(|e| {
        debug!("Failed to configure the UART: {e}");
        RebootReason::InternalError
    })?;

    // SAFETY - We only get the appended payload from here, once. It is mapped and the linker
    // script prevents it from overlapping with other objects.
    let appended_data = unsafe { get_appended_data_slice() };

    // Up to this point, we were using the built-in static (from .rodata) page tables.

    let mut page_table = mmu::PageTable::from_static_layout().map_err(|e| {
        error!("Failed to set up the dynamic page tables: {e}");
        RebootReason::InternalError
    })?;

    const CONSOLE_LEN: usize = 6;
    let uart_range = console::BASE_ADDRESS..(console::BASE_ADDRESS + CONSOLE_LEN);
    page_table.map_device(&uart_range).map_err(|e| {
        error!("Failed to remap the UART as a dynamic page table entry: {e}");
        RebootReason::InternalError
    })?;

    // SAFETY - We only get the appended payload from here, once. It is statically mapped and the
    // linker script prevents it from overlapping with other objects.
    let mut appended = unsafe { AppendedPayload::new(appended_data) }.ok_or_else(|| {
        error!("No valid configuration found");
        RebootReason::InvalidConfig
    })?;

    let bcc = appended.get_bcc_mut().ok_or_else(|| {
        error!("Invalid BCC");
        RebootReason::InvalidBcc
    })?;

    debug!("Activating dynamic page table...");
    // SAFETY - page_table duplicates the static mappings for everything that the Rust code is
    // aware of so activating it shouldn't have any visible effect.
    unsafe { page_table.activate() };
    debug!("... Success!");

    let mut memory = MemoryTracker::new(&mut page_table);
    let slices = MemorySlices::new(fdt, payload, payload_size, &mut memory)?;

    // This wrapper allows main() to be blissfully ignorant of platform details.
    crate::main(slices.fdt, slices.kernel, slices.ramdisk, bcc);

    // TODO: Overwrite BCC before jumping to payload to avoid leaking our sealing key.

    mmio_guard::unmap(console::BASE_ADDRESS).map_err(|e| {
        error!("Failed to unshare the UART: {e}");
        RebootReason::InternalError
    })?;

    Ok(())
}

fn jump_to_payload(fdt_address: u64, payload_start: u64) -> ! {
    const SCTLR_EL1_RES1: usize = (0b11 << 28) | (0b101 << 20) | (0b1 << 11);
    // Stage 1 instruction access cacheability is unaffected.
    const SCTLR_EL1_I: usize = 0b1 << 12;
    // SETEND instruction disabled at EL0 in aarch32 mode.
    const SCTLR_EL1_SED: usize = 0b1 << 8;
    // Various IT instructions are disabled at EL0 in aarch32 mode.
    const SCTLR_EL1_ITD: usize = 0b1 << 7;

    const SCTLR_EL1_VAL: usize = SCTLR_EL1_RES1 | SCTLR_EL1_ITD | SCTLR_EL1_SED | SCTLR_EL1_I;

    // SAFETY - We're exiting pvmfw by passing the register values we need to a noreturn asm!().
    unsafe {
        asm!(
            "msr sctlr_el1, {sctlr_el1_val}",
            "isb",
            "mov x1, xzr",
            "mov x2, xzr",
            "mov x3, xzr",
            "mov x4, xzr",
            "mov x5, xzr",
            "mov x6, xzr",
            "mov x7, xzr",
            "mov x8, xzr",
            "mov x9, xzr",
            "mov x10, xzr",
            "mov x11, xzr",
            "mov x12, xzr",
            "mov x13, xzr",
            "mov x14, xzr",
            "mov x15, xzr",
            "mov x16, xzr",
            "mov x17, xzr",
            "mov x18, xzr",
            "mov x19, xzr",
            "mov x20, xzr",
            "mov x21, xzr",
            "mov x22, xzr",
            "mov x23, xzr",
            "mov x24, xzr",
            "mov x25, xzr",
            "mov x26, xzr",
            "mov x27, xzr",
            "mov x28, xzr",
            "mov x29, xzr",
            "msr ttbr0_el1, xzr",
            "isb",
            "dsb nsh",
            "br x30",
            sctlr_el1_val = in(reg) SCTLR_EL1_VAL,
            in("x0") fdt_address,
            in("x30") payload_start,
            options(nomem, noreturn, nostack),
        );
    };
}

unsafe fn get_appended_data_slice() -> &'static mut [u8] {
    let base = helpers::align_up(layout::binary_end(), helpers::SIZE_4KB).unwrap();
    // pvmfw is contained in a 2MiB region so the payload can't be larger than the 2MiB alignment.
    let size = helpers::align_up(base, helpers::SIZE_2MB).unwrap() - base;

    slice::from_raw_parts_mut(base as *mut u8, size)
}

enum AppendedPayload<'a> {
    /// Configuration data.
    Config(config::Config<'a>),
    /// Deprecated raw BCC, as used in Android T.
    LegacyBcc(&'a mut [u8]),
}

impl<'a> AppendedPayload<'a> {
    /// SAFETY - 'data' should respect the alignment of config::Header.
    unsafe fn new(data: &'a mut [u8]) -> Option<Self> {
        if Self::is_valid_config(data) {
            Some(Self::Config(config::Config::new(data).unwrap()))
        } else if cfg!(feature = "legacy") {
            const BCC_SIZE: usize = helpers::SIZE_4KB;
            warn!("Assuming the appended data at {:?} to be a raw BCC", data.as_ptr());
            Some(Self::LegacyBcc(&mut data[..BCC_SIZE]))
        } else {
            None
        }
    }

    unsafe fn is_valid_config(data: &mut [u8]) -> bool {
        // This function is necessary to prevent the borrow checker from getting confused
        // about the ownership of data in new(); see https://users.rust-lang.org/t/78467.
        let addr = data.as_ptr();
        config::Config::new(data)
            .map_err(|e| warn!("Invalid configuration data at {addr:?}: {e}"))
            .is_ok()
    }

    fn get_debug_policy(&mut self) -> Option<&mut [u8]> {
        match self {
            Self::Config(ref mut cfg) => cfg.get_debug_policy(),
            Self::LegacyBcc(_) => None,
        }
    }

    #[allow(dead_code)] // TODO(b/232900974)
    fn get_bcc_mut(&mut self) -> Option<&mut [u8]> {
        let bcc = match self {
            Self::LegacyBcc(ref mut bcc) => bcc,
            Self::Config(ref mut cfg) => cfg.get_bcc_mut(),
        };
        // TODO(b/256148034): return None if BccHandoverParse(bcc) != kDiceResultOk.
        Some(bcc)
    }
}
