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

#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]

use crate::config;
use crate::fdt;
use crate::memory;
use crate::uefi;
use crate::uefi::SYSTEM_TABLE;
use core::arch::asm;
use core::mem;
use core::mem::{drop, size_of};
use core::num::NonZeroUsize;
use core::ops::Range;
use core::ptr::addr_of_mut;
use core::slice;
use hypervisor_backends::get_mem_sharer;
use hypervisor_backends::get_mmio_guard;
use log::debug;
use log::error;
use log::info;
use log::warn;
use log::LevelFilter;
use pvmfw_avb::verify_payload;
use pvmfw_avb::Capability;
use pvmfw_embedded_key::PUBLIC_KEY;
use uefi_raw::table::system::SystemTable;
use vmbase::util::RangeExt as _;
use vmbase::{
    arch::aarch64::min_dcache_line_size,
    configure_heap, console_writeln,
    layout::{self, crosvm, UART_PAGE_ADDR},
    main,
    memory::{MemoryTracker, MEMORY, SIZE_128KB, SIZE_4KB},
    power::reboot,
};
use zeroize::Zeroize;

pub const EFI_IMAGE_HANDLE: usize = 0x19ef_781b;
pub static mut PAYLOAD_SIZE: usize = 0;
pub static mut PAYLOAD_START: usize = 0;
pub static mut KERNEL_START: usize = 0;
pub static mut INITRD_START: usize = 0;

#[derive(Debug, Clone)]
pub enum RebootReason {
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
    /// Failed to verify the payload.
    PayloadVerificationError,
    /// DICE layering process failed.
    SecretDerivationError,
}

impl RebootReason {
    pub fn as_avf_reboot_string(&self) -> &'static str {
        match self {
            Self::InvalidBcc => "PVM_FIRMWARE_INVALID_BCC",
            Self::InvalidConfig => "PVM_FIRMWARE_INVALID_CONFIG_DATA",
            Self::InternalError => "PVM_FIRMWARE_INTERNAL_ERROR",
            Self::InvalidFdt => "PVM_FIRMWARE_INVALID_FDT",
            Self::InvalidPayload => "PVM_FIRMWARE_INVALID_PAYLOAD",
            Self::InvalidRamdisk => "PVM_FIRMWARE_INVALID_RAMDISK",
            Self::PayloadVerificationError => "PVM_FIRMWARE_PAYLOAD_VERIFICATION_FAILED",
            Self::SecretDerivationError => "PVM_FIRMWARE_SECRET_DERIVATION_FAILED",
        }
    }
}

main!(start);
configure_heap!(SIZE_128KB);

/// Entry point for pVM firmware.
pub fn start(fdt_address: u64, payload_start: u64, payload_size: u64, _arg3: u64) {
    // Limitations in this function:
    // - can't access non-pvmfw memory (only statically-mapped memory)
    // - can't access MMIO (except the console, already configured by vmbase)

    match main_wrapper(fdt_address as usize, payload_start as usize, payload_size as usize) {
        Ok((entry, size, bcc, use_uefi)) => {
            if use_uefi {
                info!("Jumped to EFI stub!");
                jump_to_payload_with_efi_stub(entry, size);
            } else {
                jump_to_payload(fdt_address, entry.try_into().unwrap(), bcc);
            }
        }
        Err(e) => {
            const REBOOT_REASON_CONSOLE: usize = 1;
            console_writeln!(REBOOT_REASON_CONSOLE, "{}", e.as_avf_reboot_string());
            reboot()
        }
    }

    // if we reach this point and return, vmbase::entry::rust_entry() will call power::shutdown().
}

struct MemorySlices<'a> {
    fdt: &'a mut libfdt::Fdt,
    kernel: &'a [u8],
    ramdisk: Option<&'a [u8]>,
}

impl<'a> MemorySlices<'a> {
    fn new(
        fdt: usize,
        kernel: usize,
        kernel_size: usize,
        vm_dtbo: Option<&mut [u8]>,
        vm_ref_dt: Option<&[u8]>,
    ) -> Result<Self, RebootReason> {
        let fdt_size = NonZeroUsize::new(crosvm::FDT_MAX_SIZE).unwrap();
        // TODO - Only map the FDT as read-only, until we modify it right before jump_to_payload()
        // e.g. by generating a DTBO for a template DT in main() and, on return, re-map DT as RW,
        // overwrite with the template DT and apply the DTBO.
        let range = MEMORY.lock().as_mut().unwrap().alloc_mut(fdt, fdt_size).map_err(|e| {
            error!("Failed to allocate the FDT range: {e}");
            RebootReason::InternalError
        })?;

        // SAFETY: The tracker validated the range to be in main memory, mapped, and not overlap.
        let fdt = unsafe { slice::from_raw_parts_mut(range.start as *mut u8, range.len()) };

        let info = fdt::sanitize_device_tree(fdt, vm_dtbo, vm_ref_dt)?;
        let fdt = libfdt::Fdt::from_mut_slice(fdt).map_err(|e| {
            error!("Failed to load sanitized FDT: {e}");
            RebootReason::InvalidFdt
        })?;
        debug!("Fdt passed validation!");

        let memory_range = info.memory_range;
        debug!("Resizing MemoryTracker to range {memory_range:#x?}");
        MEMORY.lock().as_mut().unwrap().shrink(&memory_range).map_err(|e| {
            error!("Failed to use memory range value from DT: {memory_range:#x?}: {e}");
            RebootReason::InvalidFdt
        })?;

        if let Some(mem_sharer) = get_mem_sharer() {
            let granule = mem_sharer.granule().map_err(|e| {
                error!("Failed to get memory protection granule: {e}");
                RebootReason::InternalError
            })?;
            MEMORY.lock().as_mut().unwrap().init_dynamic_shared_pool(granule).map_err(|e| {
                error!("Failed to initialize dynamically shared pool: {e}");
                RebootReason::InternalError
            })?;
        } else {
            let range = info.swiotlb_info.fixed_range().ok_or_else(|| {
                error!("Pre-shared pool range not specified in swiotlb node");
                RebootReason::InvalidFdt
            })?;

            MEMORY.lock().as_mut().unwrap().init_static_shared_pool(range).map_err(|e| {
                error!("Failed to initialize pre-shared pool {e}");
                RebootReason::InvalidFdt
            })?;
        }

        let kernel_range = if let Some(r) = info.kernel_range {
            r.clone()
        } else if cfg!(feature = "legacy") {
            warn!("Failed to find the kernel range in the DT; falling back to legacy ABI");

            let kernel_size = NonZeroUsize::new(kernel_size).ok_or_else(|| {
                error!("Invalid kernel size: {kernel_size:#x}");
                RebootReason::InvalidPayload
            })?;

            MEMORY.lock().as_mut().unwrap().alloc(kernel, kernel_size).map_err(|e| {
                error!("Failed to obtain the kernel range with legacy range: {e}");
                RebootReason::InternalError
            })?
        } else {
            error!("Failed to locate the kernel from the DT");
            return Err(RebootReason::InvalidPayload);
        };

        let kernel = kernel_range.start as *const u8;
        // SAFETY: The tracker validated the range to be in main memory, mapped, and not overlap.
        let kernel = unsafe { slice::from_raw_parts(kernel, kernel_range.len()) };

        let ramdisk = if let Some(r) = info.initrd_range {
            debug!("Located ramdisk at {r:?}");
            let r = MEMORY.lock().as_mut().unwrap().alloc_range(&r).map_err(|e| {
                error!("Failed to obtain the initrd range: {e}");
                RebootReason::InvalidRamdisk
            })?;

            // SAFETY: The region was validated by memory to be in main memory, mapped, and
            // not overlap.
            Some(unsafe { slice::from_raw_parts(r.start as *const u8, r.len()) })
        } else {
            info!("Couldn't locate the ramdisk from the device tree");
            None
        };

        // Set static values for payload start and size which will be used in the EFI stub.
        // Safety: TODO(nikolinailic).
        unsafe {
            KERNEL_START = kernel.as_ptr() as usize;
            INITRD_START = ramdisk.as_slice().as_ptr() as usize;
        }

        Ok(Self { fdt, kernel, ramdisk })
    }
}

/// Sets up the environment for main() and wraps its result for start().
///
/// Provide the abstractions necessary for start() to abort the pVM boot and for main() to run with
/// the assumption that its environment has been properly configured.
fn main_wrapper(
    fdt: usize,
    payload: usize,
    payload_size: usize,
) -> Result<(usize, usize, Range<usize>, bool), RebootReason> {
    // Limitations in this function:
    // - only access MMIO once (and while) it has been mapped and configured
    // - only perform logging once the logger has been initialized
    // - only access non-pvmfw memory once (and while) it has been mapped

    log::set_max_level(LevelFilter::Debug);

    let page_table = memory::init_page_table().map_err(|e| {
        error!("Failed to set up the dynamic page tables: {e}");
        RebootReason::InternalError
    })?;

    // SAFETY: We only get the appended payload from here, once. The region was statically mapped,
    // then remapped by `init_page_table()`.
    let appended_data = unsafe { get_appended_data_slice() };

    let appended = AppendedPayload::new(appended_data).ok_or_else(|| {
        error!("No valid configuration found");
        RebootReason::InvalidConfig
    })?;

    let config_entries = appended.get_entries();

    // Up to this point, we were using the built-in static (from .rodata) page tables.
    MEMORY.lock().replace(MemoryTracker::new(
        page_table,
        crosvm::MEM_START..layout::MAX_VIRT_ADDR,
        crosvm::MMIO_RANGE,
        Some(memory::appended_payload_range()),
    ));

    let slices = memory::MemorySlices::new(
        fdt,
        payload,
        payload_size,
        config_entries.vm_dtbo,
        config_entries.vm_ref_dt,
    )?;

    // This wrapper allows main() to be blissfully ignorant of platform details.
    let (next_bcc, debuggable_payload, use_uefi) = crate::main(
        slices.fdt,
        slices.kernel,
        slices.ramdisk,
        config_entries.bcc,
        config_entries.debug_policy,
    )?;

    // Writable-dirty regions will be flushed when MemoryTracker is dropped.
    config_entries.bcc.zeroize();

    info!("Expecting a bug making MMIO_GUARD_UNMAP return NOT_SUPPORTED on success");
    MEMORY.lock().as_mut().unwrap().unshare_all_mmio().map_err(|e| {
        error!("Failed to unshare MMIO ranges: {e}");
        RebootReason::InternalError
    })?;
    // Call unshare_all_memory here (instead of relying on the dtor) while UART is still mapped.
    MEMORY.lock().as_mut().unwrap().unshare_all_memory();

    if let Some(mmio_guard) = get_mmio_guard() {
        if cfg!(debuggable_vms_improvements) && debuggable_payload {
            // Keep UART MMIO_GUARD-ed for debuggable payloads, to enable earlycon.
        } else {
            mmio_guard.unmap(UART_PAGE_ADDR).map_err(|e| {
                error!("Failed to unshare the UART: {e}");
                RebootReason::InternalError
            })?;
        }
    }

    Ok((slices.kernel.as_ptr() as usize, slices.kernel.len(), next_bcc, use_uefi))
}

fn jump_to_payload(fdt_address: u64, payload_start: u64, bcc: Range<usize>) -> ! {
    const ASM_STP_ALIGN: usize = size_of::<u64>() * 2;
    const SCTLR_EL1_RES1: u64 = (0b11 << 28) | (0b101 << 20) | (0b1 << 11);
    // Stage 1 instruction access cacheability is unaffected.
    const SCTLR_EL1_I: u64 = 0b1 << 12;
    // SETEND instruction disabled at EL0 in aarch32 mode.
    const SCTLR_EL1_SED: u64 = 0b1 << 8;
    // Various IT instructions are disabled at EL0 in aarch32 mode.
    const SCTLR_EL1_ITD: u64 = 0b1 << 7;

    const SCTLR_EL1_VAL: u64 = SCTLR_EL1_RES1 | SCTLR_EL1_ITD | SCTLR_EL1_SED | SCTLR_EL1_I;

    let scratch = layout::scratch_range();

    assert_ne!(scratch.end - scratch.start, 0, "scratch memory is empty.");
    assert_eq!(scratch.start.0 % ASM_STP_ALIGN, 0, "scratch memory is misaligned.");
    assert_eq!(scratch.end.0 % ASM_STP_ALIGN, 0, "scratch memory is misaligned.");

    assert!(bcc.is_within(&(scratch.start.0..scratch.end.0)));
    assert_eq!(bcc.start % ASM_STP_ALIGN, 0, "Misaligned guest BCC.");
    assert_eq!(bcc.end % ASM_STP_ALIGN, 0, "Misaligned guest BCC.");

    let stack = memory::stack_range();

    assert_ne!(stack.end - stack.start, 0, "stack region is empty.");
    assert_eq!(stack.start.0 % ASM_STP_ALIGN, 0, "Misaligned stack region.");
    assert_eq!(stack.end.0 % ASM_STP_ALIGN, 0, "Misaligned stack region.");

    // Zero all memory that could hold secrets and that can't be safely written to from Rust.
    // Disable the exception vector, caches and page table and then jump to the payload at the
    // given address, passing it the given FDT pointer.
    //
    // SAFETY: We're exiting pvmfw by passing the register values we need to a noreturn asm!().
    unsafe {
        asm!(
            "cmp {scratch}, {bcc}",
            "b.hs 1f",

            // Zero .data & .bss until BCC.
            "0: stp xzr, xzr, [{scratch}], 16",
            "cmp {scratch}, {bcc}",
            "b.lo 0b",

            "1:",
            // Skip BCC.
            "mov {scratch}, {bcc_end}",
            "cmp {scratch}, {scratch_end}",
            "b.hs 1f",

            // Keep zeroing .data & .bss.
            "0: stp xzr, xzr, [{scratch}], 16",
            "cmp {scratch}, {scratch_end}",
            "b.lo 0b",

            "1:",
            // Flush d-cache over .data & .bss (including BCC).
            "0: dc cvau, {cache_line}",
            "add {cache_line}, {cache_line}, {dcache_line_size}",
            "cmp {cache_line}, {scratch_end}",
            "b.lo 0b",

            "mov {cache_line}, {stack}",
            // Zero stack region.
            "0: stp xzr, xzr, [{stack}], 16",
            "cmp {stack}, {stack_end}",
            "b.lo 0b",

            // Flush d-cache over stack region.
            "0: dc cvau, {cache_line}",
            "add {cache_line}, {cache_line}, {dcache_line_size}",
            "cmp {cache_line}, {stack_end}",
            "b.lo 0b",

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
            // Ensure that CMOs have completed before entering payload.
            "dsb nsh",
            "br x30",
            sctlr_el1_val = in(reg) SCTLR_EL1_VAL,
            bcc = in(reg) u64::try_from(bcc.start).unwrap(),
            bcc_end = in(reg) u64::try_from(bcc.end).unwrap(),
            cache_line = in(reg) u64::try_from(scratch.start.0).unwrap(),
            scratch = in(reg) u64::try_from(scratch.start.0).unwrap(),
            scratch_end = in(reg) u64::try_from(scratch.end.0).unwrap(),
            stack = in(reg) u64::try_from(stack.start.0).unwrap(),
            stack_end = in(reg) u64::try_from(stack.end.0).unwrap(),
            dcache_line_size = in(reg) u64::try_from(min_dcache_line_size()).unwrap(),
            in("x0") fdt_address,
            in("x30") payload_start,
            options(noreturn),
        );
    };
}

/// ARM specific.
///
/// Kernel image header format for the EFI payloads:
///
/// __HEAD
///
/// efi_signature_nop   // special NOP to identity as PE/COFF executable
/// b    primary_entry   // branch to kernel start, magic
/// .quad    0   // Image load offset from start of RAM, little-endian
/// le64sym   _kernel_size_le    // Effective size of kernel image, little-endian
/// le64sym   _kernel_flags_le    // Informative flags, little-endian
/// .quad   0   // reserved
/// .quad   0   // reserved
/// .quad   0   // reserved
/// .ascii   ARM64_IMAGE_MAGIC   // Magic number
/// .long   .Lpe_header_offset  // Offset to the PE header.
///
/// __EFI_PE_HEADER
///
///
/// EFI PE header format:
///
/// .macro   __EFI_PE_HEADER
///
/// #ifdef CONFIG_EFI
///
/// .set   .Lpe_header_offset, . - .L_head
/// .long   PE_MAGIC
/// .short   IMAGE_FILE_MACHINE_ARM64   // Machine
/// .short   .Lsection_count    // NumberOfSections
/// .long   0   // TimeDateStamp
/// .long   0   // PointerToSymbolTable
/// .long   0   // NumberOfSymbols
/// .short   .Lsection_table - .Loptional_header    // SizeOfOptionalHeader
/// .short   IMAGE_FILE_DEBUG_STRIPPED | \
///     IMAGE_FILE_EXECUTABLE_IMAGE | \
///     IMAGE_FILE_LINE_NUMS_STRIPPED   // Characteristics
///
/// .Loptional_header:
/// .short   PE_OPT_MAGIC_PE32PLUS  // PE32+ format
/// .byte   0x02    // MajorLinkerVersion
/// .byte   0x14    // MinorLinkerVersion
/// .long   __initdata_begin - .Lefi_header_end     // SizeOfCode
/// .long   __pecoff_data_size  // SizeOfInitializedData
/// .long   0   // SizeOfUninitializedData
/// .long   __efistub_efi_pe_entry - .L_head    // AddressOfEntryPoint
/// ...
/// #endif
fn jump_to_payload_with_efi_stub(payload_start: usize, payload_size: usize) {
    // SAFETY: TODO(nikolinailic).
    let system_table: *mut SystemTable = unsafe { addr_of_mut!(SYSTEM_TABLE) };
    info!("Start of the header of the payload (address): {payload_start:#x}");

    const KERNEL_HEADER_SIZE: usize = 64;
    const PE_HEADER_SIZE: usize = 24;
    const PE_MAGIC: u32 = 0x4550;
    const PE_OPT_MAGIC_PE32PLUS: u16 = 0x020b;
    let pe_header = payload_start + KERNEL_HEADER_SIZE;
    // Safety: TODO(nikolinailic).
    let pe_magic = unsafe { *(pe_header as *const u32) };
    // Sanity check: verify the PE MAGIC constant value is as defined in the UEFI spec.
    if pe_magic != PE_MAGIC {
        error!("PE MAGIC is not correct: {pe_magic:#x}, expected: {PE_MAGIC:#x}");
    }

    let pe_opt_header = pe_header + PE_HEADER_SIZE;
    // Safety: TODO(nikolinailic).
    let pe_opt_magic_pe32plus = unsafe { *(pe_opt_header as *const u16) };
    if pe_opt_magic_pe32plus != PE_OPT_MAGIC_PE32PLUS {
        error!("PE MAGIC PE32+ is not correct: {pe_opt_magic_pe32plus:#x}");
    }

    const PE32PLUS_FIELD_AOEP: usize = 16;
    let pe_ep_offset_field = pe_opt_header + PE32PLUS_FIELD_AOEP;
    // Safety: TODO(nikolinailic).
    let pe_ep_offset = usize::try_from(unsafe { *(pe_ep_offset_field as *const u32) }).unwrap();
    if pe_ep_offset >= payload_size {
        error!("Offset out of bounds: {pe_ep_offset:#x} (payload size: {payload_size:#x})");
    }

    // EFI entry point offset.
    let efi_stub_payload_start = payload_start + pe_ep_offset;

    info!("EFI Payload Start Address: {:#x}", efi_stub_payload_start);

    let efi_entry: extern "efiapi" fn(
        image_handle: usize,
        system_table: *mut SystemTable,
    ) -> uefi_raw::Status =
    // Safety: TODO(nikolinailic).
    unsafe { mem::transmute(efi_stub_payload_start) };

    // Set static values for payload start and size which will be used in the EFI stub.
    // Safety: TODO(nikolinailic).
    unsafe {
        PAYLOAD_START = payload_start;
        PAYLOAD_SIZE = payload_size;
    }
    // // SAFETY: TODO(nikolinailic).
    // unsafe {
    //     info!("Payload START VALUE: {PAYLOAD_START:#x}");
    // }

    // efi_entry(image_handle, system_table)
    let status = efi_entry(EFI_IMAGE_HANDLE, system_table);
    error!("EFI payload returned: {:?}", status);
}

/// # Safety
///
/// This must only be called once, since we are returning a mutable reference.
/// The appended data region must be mapped.
unsafe fn get_appended_data_slice() -> &'static mut [u8] {
    let range = memory::appended_payload_range();
    // SAFETY: This region is mapped and the linker script prevents it from overlapping with other
    // objects.
    unsafe { slice::from_raw_parts_mut(range.start.0 as *mut u8, range.end - range.start) }
}

enum AppendedPayload<'a> {
    /// Configuration data.
    Config(config::Config<'a>),
    /// Deprecated raw BCC, as used in Android T.
    LegacyBcc(&'a mut [u8]),
}

impl<'a> AppendedPayload<'a> {
    fn new(data: &'a mut [u8]) -> Option<Self> {
        // The borrow checker gets confused about the ownership of data (see inline comments) so we
        // intentionally obfuscate it using a raw pointer; see a similar issue (still not addressed
        // in v1.77) in https://users.rust-lang.org/t/78467.
        let data_ptr = data as *mut [u8];

        // Config::new() borrows data as mutable ...
        match config::Config::new(data) {
            // ... so this branch has a mutable reference to data, from the Ok(Config<'a>). But ...
            Ok(valid) => Some(Self::Config(valid)),
            // ... if Config::new(data).is_err(), the Err holds no ref to data. However ...
            Err(config::Error::InvalidMagic) if cfg!(feature = "legacy") => {
                // ... the borrow checker still complains about a second mutable ref without this.
                // SAFETY: Pointer to a valid mut (not accessed elsewhere), 'a lifetime re-used.
                let data: &'a mut _ = unsafe { &mut *data_ptr };

                const BCC_SIZE: usize = SIZE_4KB;
                warn!("Assuming the appended data at {:?} to be a raw BCC", data.as_ptr());
                Some(Self::LegacyBcc(&mut data[..BCC_SIZE]))
            }
            Err(e) => {
                error!("Invalid configuration data at {data_ptr:?}: {e}");
                None
            }
        }
    }

    fn get_entries(self) -> config::Entries<'a> {
        match self {
            Self::Config(cfg) => cfg.get_entries(),
            Self::LegacyBcc(bcc) => config::Entries { bcc, ..Default::default() },
        }
    }
}
