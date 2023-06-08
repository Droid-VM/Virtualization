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

//! Functions to scan the PCI bus for VirtIO devices.

use crate::memory::{MemoryTracker, MemoryTrackerError};
use alloc::boxed::Box;
use core::fmt;
use fdtpci::PciInfo;
use once_cell::race::OnceBox;
use virtio_drivers::transport::pci::bus::PciRoot;

pub(super) static PCI_INFO: OnceBox<PciInfo> = OnceBox::new();

/// PCI errors.
#[derive(Debug, Clone)]
pub enum PciError {
    /// Attempted to initialize the PCI more than once.
    DuplicateInitialization,
    /// Failed to map PCI CAM.
    CamMapFailed(MemoryTrackerError),
    /// Failed to map PCI BAR.
    BarMapFailed(MemoryTrackerError),
}

impl fmt::Display for PciError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::DuplicateInitialization => {
                write!(f, "Attempted to initialize the PCI more than once.")
            }
            Self::CamMapFailed(e) => write!(f, "Failed to map PCI CAM: {e}"),
            Self::BarMapFailed(e) => write!(f, "Failed to map PCI BAR: {e}"),
        }
    }
}

/// Prepares to use VirtIO PCI devices.
///
/// In particular:
///
/// 1. Maps the PCI CAM and BAR range in the page table and MMIO guard.
/// 2. Stores the `PciInfo` for the VirtIO HAL to use later.
/// 3. Creates and returns a `PciRoot`.
///
/// This must only be called once; it will panic if it is called a second time.
pub fn initialise(pci_info: PciInfo, memory: &mut MemoryTracker) -> Result<PciRoot, PciError> {
    PCI_INFO.set(Box::new(pci_info.clone())).map_err(|_| PciError::DuplicateInitialization)?;

    memory.map_mmio_range(pci_info.cam_range.clone()).map_err(PciError::CamMapFailed)?;
    let bar_range = pci_info.bar_range.start as usize..pci_info.bar_range.end as usize;
    memory.map_mmio_range(bar_range).map_err(PciError::BarMapFailed)?;

    // Safety: This is the only place where we call make_pci_root, and `PCI_INFO.set` above will
    // panic if it is called a second time.
    Ok(unsafe { pci_info.make_pci_root() })
}
