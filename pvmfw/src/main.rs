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

use core::arch::asm;
use vmbase::power::reboot;
use vmbase::{eprintln, main, println};

main!(main);

const PAGE_SIZE: usize = 4 << 10;
const MAX_FDT_SIZE: usize = 2 << 20;

const fn page_align(addr: usize) -> usize {
    (addr + (PAGE_SIZE - 1)) & !(PAGE_SIZE - 1)
}

extern "C" {
    // TODO: use vmbase.linker.*;
    static data_begin: u8;
    static data_end: u8;
    static data_lma: u8;
}

/// Errors in pvmfw
#[derive(Debug)]
pub enum Error {
    /// The input device tree is not in a valid state.
    InvalidDeviceTree(libfdt::FdtError),
    /// The reserved-memory node is missing from the input device tree.
    // TODO: handle this?
    MissingReservedMemory(libfdt::FdtError),
    /// Internal error while working with the device tree.
    FailedDeviceTreeOperation(libfdt::FdtError),
}

/// Main boot flow
pub fn boot_flow(fdt_arr: &mut [u8], bcc: &[u8]) -> Result<(), Error> {
    let mut fdt = libfdt::FdtWriter::new(fdt_arr).map_err(Error::InvalidDeviceTree)?;
    fdt.open_inplace(MAX_FDT_SIZE.try_into().unwrap()).map_err(Error::FailedDeviceTreeOperation)?;

    let mem = fdt.path_offset(b"/reserved-memory").map_err(Error::MissingReservedMemory)?;
    let dice = fdt.add_subnode(mem, b"dice\0").map_err(Error::FailedDeviceTreeOperation)?;
    fdt.appendprop(dice, b"compatible\0", Some(b"google,open-dice\0"))
        .map_err(Error::FailedDeviceTreeOperation)?;
    fdt.appendprop(dice, b"no-map\0", None).map_err(Error::FailedDeviceTreeOperation)?;
    fdt.appendprop_addrrange(mem, dice, b"reg\0", bcc.as_ptr() as usize as u64, bcc.len() as u64)
        .map_err(Error::FailedDeviceTreeOperation)?;

    fdt.pack().map_err(Error::FailedDeviceTreeOperation)?;
    Ok(())
}

#[inline]
fn flush_region(reg: &[u8]) {
    const CACHE_LINE_SIZE: usize = 64;

    let addr = reg.as_ptr() as usize;
    let stop = addr + reg.len();

    for line in (addr..stop).step_by(CACHE_LINE_SIZE) {
        unsafe { asm!("dc cvau, {x}", x = in(reg) line) }
    }
}

/// Entry point for pVM firmware.
pub fn main(fdt_address: u64, payload_start: u64, payload_size: u64, arg3: u64) {
    println!("pVM firmware");
    println!(
        "fdt_address={:#018x}, payload_start={:#018x}, payload_size={:#018x}, x3={:#018x}",
        fdt_address, payload_start, payload_size, arg3,
    );

    let fdt = unsafe { core::slice::from_raw_parts_mut(fdt_address as *mut u8, MAX_FDT_SIZE) };
    let bcc = unsafe {
        let data_len = (&data_end as *const _ as usize) - (&data_begin as *const _ as usize) + 1;
        let data_lma_end = (&data_lma as *const _ as usize) + data_len;
        core::slice::from_raw_parts(page_align(data_lma_end) as *const u8, PAGE_SIZE)
    };

    if let Err(e) = boot_flow(fdt, bcc) {
        eprintln!("ERROR: {:?}", e);
        reboot(); // TODO: match error to PSCI_SYS_RESET_2
    }

    flush_region(fdt); // TODO: HAFDS if we can afford the extra 4k

    println!("Starting payload...");
    // Safe because this is a function we have implemented in assembly that matches its signature
    // here.
    unsafe {
        start_payload(fdt_address, payload_start);
    }
}

extern "C" {
    fn start_payload(fdt_address: u64, payload_start: u64) -> !;
}
