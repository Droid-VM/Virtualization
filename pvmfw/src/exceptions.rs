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
use vmbase::logger;
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

#[derive(Debug, PartialEq, Copy, Clone)]
enum Esr {
    DataAbortTranslationFault,
    DataAbortSyncExternalAbort,
    Unknown(usize),
}

impl From<usize> for Esr {
    fn from(esr: usize) -> Self {
        if esr == ESR_32BIT_EXT_DABT {
            Self::DataAbortSyncExternalAbort
        } else if esr & ESR_32BIT_TRANSL_FAULT_ISS_MASK == ESR_32BIT_TRANSL_FAULT_BASE {
            Self::DataAbortTranslationFault
        } else {
            Self::Unknown(esr)
        }
    }
}

impl fmt::Display for Esr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::DataAbortSyncExternalAbort => write!(f, "Synchronous external abort"),
            Self::DataAbortTranslationFault => write!(f, "Translation fault"),
            Self::Unknown(v) => write!(f, "Unknown exception esr={v:#08x}"),
        }
    }
}

fn handle_exception(esr: Esr, far: usize) -> Result<(), HandleExceptionError> {
    // Handle all translation faults on both read and write, and MMIO guard map
    // flagged invalid pages or blocks that caused the exception.
    match esr {
        Esr::DataAbortTranslationFault => {
            let mut locked = MEMORY.try_lock().ok_or(HandleExceptionError::PageTableUnavailable)?;
            let memory = locked.as_mut().ok_or(HandleExceptionError::PageTableNotInitialized)?;
            Ok(memory.handle_mmio_fault(far)?)
        }
        _ => Err(HandleExceptionError::UnknownException),
    }
}

#[inline]
fn handling_uart_exception(esr: Esr, far: usize) -> bool {
    esr == Esr::DataAbortSyncExternalAbort && page_4kb_of(far) == UART_PAGE
}

#[no_mangle]
extern "C" fn sync_exception_current(_elr: u64, _spsr: u64) {
    // Disable logging in exception handler to prevent unsafe writes to UART.
    let _guard = logger::suppress();
    let esr: Esr = read_sysreg!("esr_el1").into();
    let far = read_sysreg!("far_el1");

    if let Err(e) = handle_exception(esr, far) {
        // Don't print to the UART if we are handling an exception it could raise.
        if !handling_uart_exception(esr, far) {
            emergency_write_str("sync_exception_current\n");
            eprintln!("{e}");
            eprintln!("{esr}, far={far:#08x}");
        }
        reboot()
    }
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
