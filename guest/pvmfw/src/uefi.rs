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

use core::mem;
use core::ptr::null_mut;
use uefi_raw::table::system::SystemTable;
use uefi_raw::Status;
use vmbase::read_sysreg;
use vmbase::write_sysreg;

/// Represents UEFI structures used for booting the Linux kernel through EFI stub.
struct EfiLoader {}

impl EfiLoader {
    const EFI_IMAGE_HANDLE: usize = 0x19ef_781b;

    fn get_system_table_ptr() -> *mut SystemTable {
        null_mut()
    }
}

pub fn execute_efi_payload(efi_payload_start: usize) -> Status {
    let efi_entry: extern "efiapi" fn(
        image_handle: usize,
        system_table: *mut SystemTable,
    ) -> uefi_raw::Status =
    // SAFETY: 'efi_stub_payload_start' points to the valid location in memory.
    unsafe { mem::transmute(efi_payload_start) };

    // TODO(ptosi): Parse EFI header and map executable & data sections separately.
    // Until then, allow the EFI payload to run from R/W data mappings.
    const SCTLR_EL1_WXN: usize = 0x1 << 19;
    let sctlr = read_sysreg!("SCTLR_EL1");
    // SAFETY: SCTLR_EL1.WXN has no visible effect on Rust.
    unsafe {
        write_sysreg!("SCTLR_EL1", sctlr & !SCTLR_EL1_WXN);
    }

    let system_table = EfiLoader::get_system_table_ptr();
    efi_entry(EfiLoader::EFI_IMAGE_HANDLE, system_table)
}
