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
use vmbase::{eprintln, power::reboot};

#[no_mangle]
extern "C" fn sync_exception_current() {
    eprintln!("sync_exception_current");
    print_esr();
    reboot();
}

#[no_mangle]
extern "C" fn irq_current() {
    eprintln!("irq_current");
    reboot();
}

#[no_mangle]
extern "C" fn fiq_current() {
    eprintln!("fiq_current");
    reboot();
}

#[no_mangle]
extern "C" fn serr_current() {
    eprintln!("serr_current");
    print_esr();
    reboot();
}

#[no_mangle]
extern "C" fn sync_lower() {
    eprintln!("sync_lower");
    print_esr();
    reboot();
}

#[no_mangle]
extern "C" fn irq_lower() {
    eprintln!("irq_lower");
    reboot();
}

#[no_mangle]
extern "C" fn fiq_lower() {
    eprintln!("fiq_lower");
    reboot();
}

#[no_mangle]
extern "C" fn serr_lower() {
    eprintln!("serr_lower");
    print_esr();
    reboot();
}

#[inline]
fn print_esr() {
    let mut esr: u64;
    unsafe {
        asm!("mrs {esr}, esr_el1", esr = out(reg) esr);
    }
    eprintln!("esr={:#08x}", esr);
}
