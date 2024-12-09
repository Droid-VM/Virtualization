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

mod boot_services;
pub mod linux;
mod loaded_image;
mod runtime_services;
mod stdio;

use crate::entry::FIRMWARE_REVISION;
use crate::uefi::linux::DEVICE_TREE_GUID;
use crate::uefi::linux::RT_PROPERTIES_TABLE_GUID;
use crate::uefi::runtime_services::RtPropertiesTable;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem;
use core::ptr::{null, null_mut};
use runtime_services::init_rt_properties_table;
use spin::mutex::SpinMutex;
use uefi_raw::protocol::console::SimpleTextOutputProtocol;
use uefi_raw::protocol::loaded_image::LoadedImageProtocol;
use uefi_raw::table::boot::BootServices;
use uefi_raw::table::configuration::ConfigurationTable;
use uefi_raw::table::runtime::RuntimeServices;
use uefi_raw::table::system::SystemTable;
use uefi_raw::table::{Header, Revision};
use uefi_raw::Guid;
use uefi_raw::Status;
use vmbase::isb;
use vmbase::{read_sysreg, write_sysreg};

const EFI_2_100_SYSTEM_TABLE_REVISION: u32 = (2 << 16) | (100);
const EFI_SPECIFICATION_REVISION: u32 = EFI_2_100_SYSTEM_TABLE_REVISION;

static EFI_LOADER: SpinMutex<EfiLoader> = SpinMutex::new(EfiLoader::new());

/// Represents UEFI structures used for booting the Linux kernel through EFI stub.
pub struct EfiLoader {
    pub system_table: SystemTable,
    boot_services: BootServices,
    runtime_services: RuntimeServices,
    simple_text_output_protocol: SimpleTextOutputProtocol,
    pub loaded_image_protocol: LoadedImageProtocol,
    firmware_vendor: [u16; 6],
    rt_properties_table: RtPropertiesTable,
    configuration_table: Vec<ConfigurationTable>,
}

impl EfiLoader {
    pub const EFI_IMAGE_HANDLE: usize = 0x19ef_781b;
    const FIRMWARE_VENDOR: [u16; 6] = ['p' as _, 'v' as _, 'm' as _, 'f' as _, 'w' as _, '\0' as _];

    pub const fn new() -> Self {
        let system_table = SystemTable {
            header: Header {
                signature: SystemTable::SIGNATURE,
                revision: Revision(EFI_SPECIFICATION_REVISION),
                size: mem::size_of::<SystemTable>() as _,
                crc: 0,
                reserved: 0,
            },

            firmware_vendor: null(),
            firmware_revision: FIRMWARE_REVISION,

            stdin_handle: null_mut(),
            stdin: null_mut(),

            stdout_handle: null_mut(),
            stdout: null_mut(),

            stderr_handle: null_mut(),
            stderr: null_mut(),

            runtime_services: null_mut(),
            boot_services: null_mut(),

            number_of_configuration_table_entries: 0,
            configuration_table: null_mut(),
        };

        let boot_services = boot_services::init_boot_services();
        let runtime_services = runtime_services::init_runtime_services();
        let simple_text_output_protocol = stdio::init_simple_text_output_protocol();
        let loaded_image_protocol = loaded_image::init_loaded_image_protocol();
        let firmware_vendor = Self::FIRMWARE_VENDOR;
        let rt_properties_table = init_rt_properties_table();
        let configuration_table = Vec::new();

        Self {
            system_table,
            boot_services,
            runtime_services,
            simple_text_output_protocol,
            loaded_image_protocol,
            firmware_vendor,
            rt_properties_table,
            configuration_table,
        }
    }

    fn get_system_table_ptr(&mut self) -> *mut SystemTable {
        &mut self.system_table as _
    }

    fn get_rt_properties_table_ptr(&mut self) -> *mut c_void {
        &mut self.rt_properties_table as *const _ as *mut c_void
    }

    fn set_loaded_image_protocol(&mut self, payload: &[u8]) {
        self.loaded_image_protocol.image_base = payload.as_ptr().cast();
        let image_size = payload.len().try_into().unwrap();
        self.loaded_image_protocol.image_size = image_size;
    }

    fn patch_pointers(&mut self, payload: &[u8]) {
        self.system_table.boot_services = &mut self.boot_services as *mut _;
        self.system_table.runtime_services = &mut self.runtime_services as *mut _;
        self.system_table.firmware_vendor = &mut self.firmware_vendor as *mut _;
        self.system_table.stdout = &mut self.simple_text_output_protocol as *mut _;
        self.system_table.stderr = &mut self.simple_text_output_protocol as *mut _;
        self.loaded_image_protocol.system_table = &mut self.system_table as *mut _;

        self.set_loaded_image_protocol(payload);
    }
}

// SAFETY: Send trait indicates whether a type is safe to transfer across threads. We are working
// with single-threaded system so this operation has no meaningful impact on Rust.
unsafe impl Send for EfiLoader {}

// Initialize parameters passed to the EFI stub by the EFI loader.
pub fn init_efi(payload: &[u8]) {
    let rt_properties_table_ptr = EFI_LOADER.lock().get_rt_properties_table_ptr();
    push_to_config_table(RT_PROPERTIES_TABLE_GUID, rt_properties_table_ptr);
    push_to_config_table(DEVICE_TREE_GUID, fdt_address as *mut c_void);

    EFI_LOADER.lock().patch_pointers(payload);
}

fn push_to_config_table(vendor_guid: Guid, vendor_table: *mut c_void) {
    let configuration_table_entry = ConfigurationTable { vendor_guid, vendor_table };
    let mut efi_loader = EFI_LOADER.lock();
    efi_loader.configuration_table.push(configuration_table_entry);
    efi_loader.system_table.configuration_table = efi_loader.configuration_table.as_mut_ptr();
    efi_loader.system_table.number_of_configuration_table_entries =
        efi_loader.configuration_table.len();
}

pub type EfiEntrypoint = extern "efiapi" fn(usize, *mut SystemTable) -> uefi_raw::Status;

pub fn execute_efi_payload(entrypoint: EfiEntrypoint) -> Status {
    // TODO(ptosi): Parse EFI header and map executable & data sections separately.
    // Until then, allow the EFI payload to run from R/W data mappings.
    const SCTLR_EL1_WXN: usize = 0x1 << 19;
    let sctlr = read_sysreg!("SCTLR_EL1");
    let sctlr = sctlr & !SCTLR_EL1_WXN;
    // SAFETY: SCTLR_EL1.WXN has no visible effect on Rust.
    unsafe { write_sysreg!("SCTLR_EL1", sctlr) };

    isb!();

    let system_table = EFI_LOADER.lock().get_system_table_ptr();
    entrypoint(EfiLoader::EFI_IMAGE_HANDLE, system_table)
}
