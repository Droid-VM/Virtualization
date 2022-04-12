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

//! pVM firmware.

#![no_main]
#![no_std]

mod exceptions;

use vmbase::{console, power::shutdown, println};

static ZEROED_DATA: [u32; 10] = [0; 10];
static INITIALISED_DATA: [u32; 4] = [1, 2, 3, 4];
static mut MUTABLE_DATA: [u32; 4] = [1, 2, 3, 4];

/// Entry point for pVM firmware.
#[no_mangle]
pub extern "C" fn main() -> ! {
    console::init();
    println!("Hello world");
    print_addresses();
    check_data();

    shutdown();
}

fn print_addresses() {
    unsafe {
        println!(
            "dtb: {:#08x}-{:#08x}",
            &dtb_begin as *const u8 as usize, &dtb_end as *const u8 as usize
        );
        println!(
            "text: {:#08x}-{:#08x}",
            &text_begin as *const u8 as usize, &text_end as *const u8 as usize
        );
        println!(
            "rodata: {:#08x}-{:#08x}",
            &rodata_begin as *const u8 as usize, &rodata_end as *const u8 as usize
        );
        println!(
            "data: {:#08x}-{:#08x} (loaded at {:#08x})",
            &data_begin as *const u8 as usize,
            &data_end as *const u8 as usize,
            &data_lma as *const u8 as usize,
        );
        println!(
            "bss: {:#08x}-{:#08x}",
            &bss_begin as *const u8 as usize, &bss_end as *const u8 as usize
        );
        println!(
            "boot_stack: {:#08x}-{:#08x}",
            &boot_stack_begin as *const u8 as usize, &boot_stack_end as *const u8 as usize
        );
    }
}

fn check_data() {
    println!("ZEROED_DATA: {:#08x}", &ZEROED_DATA as *const u32 as usize);
    println!("INITIALISED_DATA: {:#08x}", &INITIALISED_DATA as *const u32 as usize);
    unsafe {
        println!("MUTABLE_DATA: {:#08x}", &MUTABLE_DATA as *const u32 as usize);
    }

    for element in ZEROED_DATA.iter() {
        assert_eq!(*element, 0);
    }
    assert_eq!(INITIALISED_DATA[0], 1);
    assert_eq!(INITIALISED_DATA[1], 2);
    assert_eq!(INITIALISED_DATA[2], 3);
    assert_eq!(INITIALISED_DATA[3], 4);
    unsafe {
        assert_eq!(MUTABLE_DATA[0], 1);
        assert_eq!(MUTABLE_DATA[1], 2);
        assert_eq!(MUTABLE_DATA[2], 3);
        assert_eq!(MUTABLE_DATA[3], 4);
        MUTABLE_DATA[0] += 41;
        assert_eq!(MUTABLE_DATA[0], 42);
    }
    println!("Data looks good");
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
