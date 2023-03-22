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

//! Exception handlers.

use crate::helpers::page_4kb_of;
use crate::memory::{handle_flagged_page_fault, handle_permission_fault, MEMORY};
use aarch64_paging::paging::PteUpdater;
use core::arch::asm;
use core::ops::Range;
use vmbase::console;
use vmbase::{console::emergency_write_str, eprintln, power::reboot};

const ESR_32BIT_EXT_DABT: u64 = 0x96000010;
const UART_PAGE: usize = page_4kb_of(console::BASE_ADDRESS);
const ESR_32BIT_TRANSL_FAULT_BASE: u64 = 0x96000004;
const ESR_32BIT_TRANSL_FAULT_ISS_MASK: u64 = !0x43;
const ESR_32BIT_PERM_FAULT_BASE: u64 = 0x9600004C;
const ESR_32BIT_PERM_FAULT_ISS_MASK: u64 = !0x3;

#[no_mangle]
extern "C" fn sync_exception_current(_elr: u64, _spsr: u64) {
    let esr = read_esr();
    let far = read_far() as usize;
    // Don't print to the UART if we're handling the exception it could raise.
    if esr != ESR_32BIT_EXT_DABT || page_4kb_of(far) != UART_PAGE {
        emergency_write_str("sync_exception_current\n");
        print_esr(esr);
    }

    // Handle all translation faults on both read and write, and MMIO guard map
    // flagged invalid pages or blocks that caused the exception.
    // Handle permission faults for DBM flagged entries, and flag them as dirty on write.
    if esr & ESR_32BIT_TRANSL_FAULT_ISS_MASK == ESR_32BIT_TRANSL_FAULT_BASE
        && modify_pte_range(&(far..far + 1), &handle_flagged_page_fault).is_ok()
        || esr & ESR_32BIT_PERM_FAULT_ISS_MASK == ESR_32BIT_PERM_FAULT_BASE
            && modify_pte_range(&(far..far + 1), &handle_permission_fault).is_ok()
    {
        return;
    }
    reboot();
}

fn modify_pte_range(va_range: &Range<usize>, f: &PteUpdater) -> Result<(), ()> {
    let is_uart = (page_4kb_of(va_range.start)..va_range.end).contains(&UART_PAGE);
    match MEMORY.try_lock() {
        None => {
            if !is_uart {
                eprintln!("page table unavailable");
            }
        }
        Some(mut g) => match g.as_mut() {
            None => {
                if !is_uart {
                    eprintln!("page table not initialized");
                }
            }
            Some(memory) => match memory.modify_range(va_range, f) {
                Err(e) => {
                    if !is_uart {
                        eprintln!("page table update error: {}", e);
                    }
                }
                _ => return Ok(()),
            },
        },
    }
    Err(())
}

#[no_mangle]
extern "C" fn irq_current(_elr: u64, _spsr: u64) {
    emergency_write_str("irq_current\n");
    reboot();
}

#[no_mangle]
extern "C" fn fiq_current(_elr: u64, _spsr: u64) {
    emergency_write_str("fiq_current\n");
    reboot();
}

#[no_mangle]
extern "C" fn serr_current(_elr: u64, _spsr: u64) {
    let esr = read_esr();
    emergency_write_str("serr_current\n");
    print_esr(esr);
    reboot();
}

#[no_mangle]
extern "C" fn sync_lower(_elr: u64, _spsr: u64) {
    let esr = read_esr();
    emergency_write_str("sync_lower\n");
    print_esr(esr);
    reboot();
}

#[no_mangle]
extern "C" fn irq_lower(_elr: u64, _spsr: u64) {
    emergency_write_str("irq_lower\n");
    reboot();
}

#[no_mangle]
extern "C" fn fiq_lower(_elr: u64, _spsr: u64) {
    emergency_write_str("fiq_lower\n");
    reboot();
}

#[no_mangle]
extern "C" fn serr_lower(_elr: u64, _spsr: u64) {
    let esr = read_esr();
    emergency_write_str("serr_lower\n");
    print_esr(esr);
    reboot();
}

#[inline]
fn read_esr() -> u64 {
    let mut esr: u64;
    unsafe {
        asm!("mrs {esr}, esr_el1", esr = out(reg) esr);
    }
    esr
}

#[inline]
fn print_esr(esr: u64) {
    eprintln!("esr={:#08x}", esr);
}

#[inline]
fn read_far() -> u64 {
    let mut far: u64;
    unsafe {
        asm!("mrs {far}, far_el1", far = out(reg) far);
    }
    far
}
