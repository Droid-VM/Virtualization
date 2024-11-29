// Copyright 2023, The Android Open Source Project
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

//! Helper functions and structs for exception handlers.

use crate::memory::{MemoryTrackerError, MEMORY};
use aarch64_paging::paging::VirtualAddress;
use core::fmt;
use core::result;

/// Represents an error that can occur while handling an exception.
#[derive(Debug)]
pub enum HandleExceptionError {
    /// The page table is unavailable.
    PageTableUnavailable,
    /// The page table has not been initialized.
    PageTableNotInitialized,
    /// An internal error occurred in the memory tracker.
    InternalError(MemoryTrackerError),
    /// An unknown exception occurred.
    UnknownException,
}

impl From<MemoryTrackerError> for HandleExceptionError {
    fn from(other: MemoryTrackerError) -> Self {
        Self::InternalError(other)
    }
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

/// Handles a translation fault with the given fault address register (FAR).
#[inline]
pub fn handle_translation_fault(far: VirtualAddress) -> result::Result<(), HandleExceptionError> {
    let mut guard = MEMORY.try_lock().ok_or(HandleExceptionError::PageTableUnavailable)?;
    let memory = guard.as_mut().ok_or(HandleExceptionError::PageTableNotInitialized)?;
    Ok(memory.handle_mmio_fault(far)?)
}

/// Handles a permission fault with the given fault address register (FAR).
#[inline]
pub fn handle_permission_fault(far: VirtualAddress) -> result::Result<(), HandleExceptionError> {
    let mut guard = MEMORY.try_lock().ok_or(HandleExceptionError::PageTableUnavailable)?;
    let memory = guard.as_mut().ok_or(HandleExceptionError::PageTableNotInitialized)?;
    Ok(memory.handle_permission_fault(far)?)
}
