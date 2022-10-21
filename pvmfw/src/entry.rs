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

use crate::helpers::FDT_MAX_SIZE;
use core::arch::asm;
use core::{fmt, slice};
use log::LevelFilter;
use vmbase::{logger, main, power::reboot};

#[derive(Debug, Clone)]
enum RebootReason {
    /// An unexpected internal error happened.
    InternalError,
}

impl fmt::Display for RebootReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InternalError => write!(f, "Internal error"),
        }
    }
}

main!(start);
/// Entry point for pVM firmware.
pub fn start(fdt_address: u64, payload_start: u64, payload_size: u64, _arg3: u64) {
    // Limitations in this function:
    // - can't access non-pvmfw memory (only statically-mapped memory)
    // - can't access MMIO (therefore, no logging)
    // - deal with raw ABIs

    // SAFETY - These slices will be validated by main_wrapper().
    let fdt = unsafe { slice::from_raw_parts_mut(fdt_address as usize as *mut u8, FDT_MAX_SIZE) };
    let payload = unsafe {
        slice::from_raw_parts(payload_start as usize as *const u8, payload_size as usize)
    };

    match main_wrapper(fdt, payload) {
        Ok(_) => jump_to_payload(fdt_address, payload_start),
        Err(_) => reboot(),
    }

    // if we reach this point and return, vmbase::entry::rust_entry() will call power::shutdown().
}

/// Sets up the environment for main() and wraps its result for start().
///
/// Provide the abstractions necessary for start() to abort the pVM boot and for main() to run with
/// the assumption that its environment has been properly configured.
fn main_wrapper(fdt: &mut [u8], payload: &[u8]) -> Result<(), RebootReason> {
    // Limitations in this function:
    // - only access MMIO once (and while) it has been mapped and configured
    // - only perform logging once the logger has been initialized
    // - only access non-pvmfw memory once (and while) it has been mapped
    // - the location of inputs (in safe types) can't be trusted
    logger::init(LevelFilter::Debug).map_err(|_| RebootReason::InternalError)?;

    // This wrapper allows main() to be blissfully ignorant of platform details.
    crate::main(fdt, payload).map_err(|_| RebootReason::InternalError)?;

    Ok(())
}

fn jump_to_payload(fdt_address: u64, payload_start: u64) -> ! {
    const SCTLR_EL1_RES1: usize = (0b11 << 28) | (0b101 << 20) | (0b1 << 11);
    // Stage 1 instruction access cacheability is unaffected.
    const SCTLR_EL1_I: usize = (0b1 << 12);
    // SETEND instruction disabled at EL0 in aarch32 mode.
    const SCTLR_EL1_SED: usize = (0b1 << 8);
    // Various IT instructions are disabled at EL0 in aarch32 mode.
    const SCTLR_EL1_ITD: usize = (0b1 << 7);

    const SCTLR_EL1_VAL: usize = SCTLR_EL1_RES1 | SCTLR_EL1_ITD | SCTLR_EL1_SED | SCTLR_EL1_I;

    // SAFETY - We're exiting pvmfw by passing the register values we need to a noreturn asm!().
    unsafe {
        asm!(
            "msr sctlr_el1, {sctlr_el1_val}",
            "mov x18, xzr",
            "mov x19, xzr",
            "mov x29, xzr",
            "isb",
            "msr ttbr0_el1, xzr",
            "isb",
            "dsb nsh",
            "br x30",
            sctlr_el1_val = in(reg) SCTLR_EL1_VAL,
            in("x0") fdt_address,
            in("x1") 0,
            in("x2") 0,
            in("x3") 0,
            in("x4") 0,
            in("x5") 0,
            in("x6") 0,
            in("x7") 0,
            in("x8") 0,
            in("x9") 0,
            in("x10") 0,
            in("x11") 0,
            in("x12") 0,
            in("x13") 0,
            in("x14") 0,
            in("x15") 0,
            in("x16") 0,
            in("x17") 0,
            // x18 is a reserved register.
            // x19 is used internally by LLVM and cannot be used as an operand for inline asm.
            in("x20") 0,
            in("x21") 0,
            in("x22") 0,
            in("x23") 0,
            in("x24") 0,
            in("x25") 0,
            in("x26") 0,
            in("x27") 0,
            in("x28") 0,
            // the frame pointer cannot be used as an operand for inline asm.
            in("x30") payload_start,
            options(nomem, noreturn, nostack),
        );
    };
}
