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

use core::arch::asm;
use vmbase::{console::emergency_write_str, eprintln, power::reboot};

#[no_mangle]
extern "C" fn sync_exception_current(_elr: u64, _spsr: u64) {
    let esr = read_esr();

    if esr == 0x96000010 {
        const PAGE_MASK: u64 = (1 << 12) - 1;
        let far = read_far();
        // TODO: PTW to verify we have a stage-1 MMIO mapping
        if !share_mmio_page(far & PAGE_MASK) {
            return;
        }
    }

    emergency_write_str("sync_exception_current\n");
    print_esr(esr);
    reboot();
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

#[inline(always)]
fn share_mmio_page(page: u64) -> u64 {
    const FUNC_ID: u64 = 0xc6000007;
    let mut args = [0u64; 17];
    args[0] = page;

    hvc64(FUNC_ID, args)
}

#[inline(always)]
fn hvc64(function: u32, args: [u64; 17]) -> [u64; 18] {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let mut ret = [0; 18];

        core::arch::asm!(
            "hvc #0",
            inout("x0") function as u64 => ret[0],
            inout("x1") args[0] => ret[1],
            inout("x2") args[1] => ret[2],
            inout("x3") args[2] => ret[3],
            inout("x4") args[3] => ret[4],
            inout("x5") args[4] => ret[5],
            inout("x6") args[5] => ret[6],
            inout("x7") args[6] => ret[7],
            inout("x8") args[7] => ret[8],
            inout("x9") args[8] => ret[9],
            inout("x10") args[9] => ret[10],
            inout("x11") args[10] => ret[11],
            inout("x12") args[11] => ret[12],
            inout("x13") args[12] => ret[13],
            inout("x14") args[13] => ret[14],
            inout("x15") args[14] => ret[15],
            inout("x16") args[15] => ret[16],
            inout("x17") args[16] => ret[17],
            options(nomem, nostack)
        );

        ret
    }
}
