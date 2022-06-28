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

use aarch64_paging::paging::{MemoryRegion, VirtualAddress};
use core::ops::Range;
use vmbase::println;

/// The first 1 GiB of memory are used for MMIO.
pub const DEVICE_REGION: MemoryRegion = MemoryRegion::new(0, 0x40000000);

/// Memory reserved for the DTB.
pub fn dtb_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&dtb_begin as *const u8 as usize)
            ..VirtualAddress(&dtb_end as *const u8 as usize)
    }
}

pub fn dtb_region() -> MemoryRegion {
    let range = dtb_range();
    MemoryRegion::new(range.start.0, range.end.0)
}

/// Executable code.
pub fn text_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&text_begin as *const u8 as usize)
            ..VirtualAddress(&text_end as *const u8 as usize)
    }
}

pub fn text_region() -> MemoryRegion {
    let range = text_range();
    MemoryRegion::new(range.start.0, range.end.0)
}

/// Read-only data.
pub fn rodata_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&rodata_begin as *const u8 as usize)
            ..VirtualAddress(&rodata_end as *const u8 as usize)
    }
}

pub fn rodata_region() -> MemoryRegion {
    let range = rodata_range();
    MemoryRegion::new(range.start.0, range.end.0)
}

/// Initialised writable data.
pub fn data_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&data_begin as *const u8 as usize)
            ..VirtualAddress(&data_end as *const u8 as usize)
    }
}

/// Zero-initialised writable data.
pub fn bss_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&bss_begin as *const u8 as usize)
            ..VirtualAddress(&bss_end as *const u8 as usize)
    }
}

/// Writable data region for the stack.
pub fn boot_stack_range() -> Range<VirtualAddress> {
    unsafe {
        VirtualAddress(&boot_stack_begin as *const u8 as usize)
            ..VirtualAddress(&boot_stack_end as *const u8 as usize)
    }
}

/// Writable data, including the stack.
pub fn writable_region() -> MemoryRegion {
    unsafe {
        MemoryRegion::new(&data_begin as *const _ as usize, &boot_stack_end as *const _ as usize)
    }
}

pub fn print_addresses() {
    let dtb = dtb_range();
    println!("dtb:        {}-{} ({} bytes)", dtb.start, dtb.end, dtb.end.0 - dtb.start.0);
    let text = text_range();
    println!("text:       {}-{} ({} bytes)", text.start, text.end, text.end.0 - text.start.0);
    let rodata = rodata_range();
    println!(
        "rodata:     {}-{} ({} bytes)",
        rodata.start,
        rodata.end,
        rodata.end.0 - rodata.start.0
    );

    let data = data_range();
    unsafe {
        println!(
            "data:       {}-{} ({} bytes, loaded at {:#018x})",
            data.start,
            data.end,
            data.end.0 - data.start.0,
            &data_lma as *const u8 as usize,
        );
    }
    let bss = bss_range();
    println!("bss:        {}-{} ({} bytes)", bss.start, bss.end, bss.end.0 - bss.start.0);
    let boot_stack = boot_stack_range();
    println!(
        "boot_stack: {}-{} ({} bytes)",
        boot_stack.start,
        boot_stack.end,
        boot_stack.end.0 - boot_stack.start.0
    );
}

extern "C" {
    static dtb_begin: u8;
    static dtb_end: u8;
    static text_begin: u8;
    static text_end: u8;
    static rodata_begin: u8;
    static rodata_end: u8;
    static data_begin: u8;
    static data_end: u8;
    static data_lma: u8;
    static bss_begin: u8;
    static bss_end: u8;
    static boot_stack_begin: u8;
    static boot_stack_end: u8;
}
