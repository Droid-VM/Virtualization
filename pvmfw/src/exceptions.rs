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

use crate::memory::{MemoryTrackerError, MEMORY};
use crate::{helpers::page_4kb_of, read_sysreg};
use core::fmt;
use vmbase::console;
use vmbase::{console::emergency_write_str, eprintln, power::reboot};

const ESR_32BIT_EXT_DABT: usize = 0x96000010;
const UART_PAGE: usize = page_4kb_of(console::BASE_ADDRESS);
const ESR_32BIT_TRANSL_FAULT_BASE: usize = 0x96000004;
const ESR_32BIT_TRANSL_FAULT_ISS_MASK: usize = !0x43;

#[derive(Debug)]
enum HandleExceptionError {
    PageTableUnavailable,
    PageTableNotInitialized,
    InternalError(MemoryTrackerError),
    UnknownException,
}

impl fmt::Display for HandleExceptionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::PageTableUnavailable => write!(f, "Page table is not available."),
            Self::PageTableNotInitialized => write!(f, "Page table is not initialized."),
            Self::InternalError(e) => write!(f, "Error while updating page table: {e}"),
            Self::UnknownException => write!(f, "An unknown exception occurred, not handled."),
        }
    }
}

impl From<MemoryTrackerError> for HandleExceptionError {
    fn from(other: MemoryTrackerError) -> Self {
        Self::InternalError(other)
    }
}

fn handle_exception(esr: usize, far: usize) -> Result<(), HandleExceptionError> {
    let mut locked = MEMORY.try_lock().ok_or(HandleExceptionError::PageTableUnavailable)?;
    let memory = locked.as_mut().ok_or(HandleExceptionError::PageTableNotInitialized)?;
    // Handle all translation faults on both read and write, and MMIO guard map
    // flagged invalid pages or blocks that caused the exception.
    if esr & ESR_32BIT_TRANSL_FAULT_ISS_MASK == ESR_32BIT_TRANSL_FAULT_BASE {
        memory.handle_mmio_fault(far).map_err(|e| e.into())
    } else {
        Err(HandleExceptionError::UnknownException)
    }
}

#[no_mangle]
extern "C" fn sync_exception_current(_elr: u64, _spsr: u64) {
    let esr = read_sysreg!("esr_el1");
    let far = read_sysreg!("far_el1");
    let is_uart = page_4kb_of(far) == UART_PAGE;

    // Don't print to the UART if we're handling the exception it could raise.
    if esr != ESR_32BIT_EXT_DABT || !is_uart {
        emergency_write_str("sync_exception_current\n");
    }
    match handle_exception(esr, far) {
        Ok(()) => return,
        Err(e) if !is_uart => {
            eprintln!("{e}");
            eprintln!("esr={esr:#08x}, far={far:#08x}");
        }
        _ => (),
    }
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
    let esr = read_sysreg!("esr_el1");
    emergency_write_str("serr_current\n");
    eprintln!("esr={esr:#08x}");
    reboot();
}

#[no_mangle]
extern "C" fn sync_lower(_elr: u64, _spsr: u64) {
    let esr = read_sysreg!("esr_el1");
    emergency_write_str("sync_lower\n");
    eprintln!("esr={esr:#08x}");
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
    let esr = read_sysreg!("esr_el1");
    emergency_write_str("serr_lower\n");
    eprintln!("esr={esr:#08x}");
    reboot();
}
