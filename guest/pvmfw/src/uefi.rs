// Copyright 2024, The Android Open Source Project
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

//! Support for EFI payloads.

pub mod linux;

use core::ptr::null_mut;
use spin::mutex::SpinMutex;
use uefi_raw::table::system::SystemTable;
use uefi_raw::Handle;
use uefi_raw::Status;
use vmbase::arch::disable_wxn;

static EFI_LOADER: SpinMutex<EfiLoader> = SpinMutex::new(EfiLoader::new());

/// Represents the UEFI structures used for booting EFI payloads.
struct EfiLoader {}

impl EfiLoader {
    const EFI_IMAGE_HANDLE: Handle = 0x19ef_781b as _;

    pub const fn new() -> Self {
        Self {}
    }

    fn get_system_table_ptr(&mut self) -> *mut SystemTable {
        null_mut()
    }
}

// SAFETY: `Send` is not relevant as pvmfw is single-threaded.
unsafe impl Send for EfiLoader {}

pub type EfiEntrypoint = extern "efiapi" fn(Handle, *mut SystemTable) -> uefi_raw::Status;

pub fn execute_efi_payload(entrypoint: EfiEntrypoint) -> Status {
    // TODO(ptosi): Parse EFI header and map executable & data sections separately.
    // Until then, allow the EFI payload to run from R/W data mappings.
    disable_wxn();

    let system_table = EFI_LOADER.lock().get_system_table_ptr();
    entrypoint(EfiLoader::EFI_IMAGE_HANDLE, system_table)
}
