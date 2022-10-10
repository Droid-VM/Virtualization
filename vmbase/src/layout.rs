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

//! Memory layout.

use crate::linker;
use aarch64_paging::paging::{MemoryRegion, VirtualAddress};
use core::arch::asm;
use core::ops::Range;

/// The first 1 GiB of memory are used for MMIO.
pub const DEVICE_REGION: MemoryRegion = MemoryRegion::new(0, 0x40000000);

/// Memory reserved for the DTB.
pub fn dtb_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&linker::dtb_begin as *const u8 as usize)
            ..VirtualAddress(&linker::dtb_end as *const u8 as usize)
    }
}

/// Executable code.
pub fn text_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&linker::text_begin as *const u8 as usize)
            ..VirtualAddress(&linker::text_end as *const u8 as usize)
    }
}

/// Read-only data.
pub fn rodata_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&linker::rodata_begin as *const u8 as usize)
            ..VirtualAddress(&linker::rodata_end as *const u8 as usize)
    }
}

/// Initialised writable data.
pub fn data_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&linker::data_begin as *const u8 as usize)
            ..VirtualAddress(&linker::data_end as *const u8 as usize)
    }
}

/// Zero-initialised writable data.
pub fn bss_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&linker::bss_begin as *const u8 as usize)
            ..VirtualAddress(&linker::bss_end as *const u8 as usize)
    }
}

/// Writable data region for the stack.
pub fn boot_stack_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&linker::boot_stack_begin as *const u8 as usize)
            ..VirtualAddress(&linker::boot_stack_end as *const u8 as usize)
    }
}

/// Writable data, including the stack.
pub fn writable_region() -> MemoryRegion {
    unsafe {
        MemoryRegion::new(
            &linker::data_begin as *const u8 as usize,
            &linker::boot_stack_end as *const u8 as usize,
        )
    }
}

/// Read-write data (original).
pub fn data_load_address() -> VirtualAddress {
    unsafe { VirtualAddress(&linker::data_lma as *const u8 as usize) }
}

/// End of the binary image.
pub fn binary_end() -> VirtualAddress {
    unsafe { VirtualAddress(&linker::bin_end as *const u8 as usize) }
}

/// Bionic-compatible thread-local storage entry, at the given offset from TPIDR_EL0.
pub fn bionic_tls(off: usize) -> u64 {
    let mut base: usize;
    unsafe {
        asm!("mrs {base}, tpidr_el0", base = out(reg) base);
        let ptr = (base + off) as *const u64;
        *ptr
    }
}

/// Value of __stack_chk_guard.
pub fn stack_chk_guard() -> u64 {
    unsafe { linker::__stack_chk_guard }
}
