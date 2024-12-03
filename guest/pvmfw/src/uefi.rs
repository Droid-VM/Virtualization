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

use crate::entry::FIRMWARE_REVISION;
use crate::uefi::runtime_services::RtPropertiesTable;
use crate::RebootReason;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem;
use core::ptr::{null, null_mut};
use log::error;
use spin::mutex::SpinMutex;
use uefi_raw::protocol::console::SimpleTextOutputProtocol;
use uefi_raw::protocol::loaded_image::LoadedImageProtocol;
use uefi_raw::table::boot::BootServices;
use uefi_raw::table::configuration::ConfigurationTable;
use uefi_raw::table::runtime::RuntimeServices;
use uefi_raw::table::system::SystemTable;
use uefi_raw::table::{Header, Revision};
use uefi_raw::{guid, Guid};

const RT_PROPERTIES_TABLE_GUID: Guid = guid!("eb66918a-7eef-402a-842e-931d21c38ae9");
const DEVICE_TREE_GUID: Guid = guid!("b1b621d5-f19c-41a5-830b-d9152c69aae0");
const EFI_2_100_SYSTEM_TABLE_REVISION: u32 = (2 << 16) | (100);
const EFI_SPECIFICATION_REVISION: u32 = EFI_2_100_SYSTEM_TABLE_REVISION;

static EFI_LOADER: SpinMutex<EfiLoader> = SpinMutex::new(EfiLoader::new());
// The firmware allocated handle for the UEFI image.
pub const EFI_IMAGE_HANDLE: usize = 0x19ef_781b;

mod boot_services;
mod linux;
mod loaded_image;
mod runtime_services;
mod stdio;

/// Represents UEFI structures used for booting the Linux kernel through EFI stub.
struct EfiLoader {
    pub system_table: SystemTable,
    boot_services: BootServices,
    runtime_services: RuntimeServices,
    simple_text_output_protocol: SimpleTextOutputProtocol,
    pub loaded_image_protocol: LoadedImageProtocol,
    rt_properties_table: RtPropertiesTable,
    configuration_table: Vec<ConfigurationTable>,
    firmware_vendor: [u16; 6],
}

impl EfiLoader {
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
        let simple_text_output_protocol = stdio::init_stdio();
        let loaded_image_protocol = loaded_image::init_loaded_image_protocol();
        let rt_properties_table = runtime_services::init_rt_properties_table();
        let configuration_table = Vec::new();
        let firmware_vendor = Self::FIRMWARE_VENDOR;

        Self {
            system_table,
            boot_services,
            runtime_services,
            simple_text_output_protocol,
            loaded_image_protocol,
            rt_properties_table,
            configuration_table,
            firmware_vendor,
        }
    }

    fn patch_pointers(&mut self) {
        self.system_table.boot_services = &mut self.boot_services as *mut _;
        self.system_table.runtime_services = &mut self.runtime_services as *mut _;
        self.system_table.firmware_vendor = &mut self.firmware_vendor as *mut _;
        self.system_table.stdout = &mut self.simple_text_output_protocol as *mut _;
        self.system_table.stderr = &mut self.simple_text_output_protocol as *mut _;
        self.loaded_image_protocol.system_table = &mut self.system_table as *mut _;
    }
}

// SAFETY: TODO(nikolinailic).
unsafe impl Send for EfiLoader {}

// Initialize parameters passed to the EFI stub by the EFI loader.
pub fn init_efi(fdt_address: usize, payload_start: usize, payload_size: usize) -> *mut SystemTable {
    let rt_properties_table_ptr = get_rt_properties_table_ptr();
    push_to_config_table(RT_PROPERTIES_TABLE_GUID, rt_properties_table_ptr);
    push_to_config_table(DEVICE_TREE_GUID, fdt_address as *mut c_void);

    EFI_LOADER.lock().patch_pointers();

    set_loaded_image_protocol(payload_start, payload_size);
    get_system_table_ptr()
}

fn push_to_config_table(vendor_guid: Guid, vendor_table: *mut c_void) {
    let configuration_table_entry = ConfigurationTable { vendor_guid, vendor_table };
    let mut efi_loader = EFI_LOADER.lock();
    efi_loader.configuration_table.push(configuration_table_entry);
    efi_loader.system_table.configuration_table = efi_loader.configuration_table.as_mut_ptr();
    efi_loader.system_table.number_of_configuration_table_entries =
        efi_loader.configuration_table.len();
}

fn get_system_table_ptr() -> *mut SystemTable {
    let mut efi_loader = EFI_LOADER.lock();
    &mut efi_loader.system_table as *mut _
}

fn get_rt_properties_table_ptr() -> *mut c_void {
    let efi_loader = EFI_LOADER.lock();
    &efi_loader.rt_properties_table as *const _ as *mut c_void
}

fn set_loaded_image_protocol(payload_start: usize, payload_size: usize) {
    let mut efi_loader = EFI_LOADER.lock();
    efi_loader.loaded_image_protocol.image_base = payload_start as *const _;
    let image_size = payload_size.try_into().unwrap();
    efi_loader.loaded_image_protocol.image_size = image_size;
}

pub fn execute_efi_payload(
    efi_payload_start: usize,
    system_table: *mut SystemTable,
) -> RebootReason {
    let efi_entry: extern "efiapi" fn(
        image_handle: usize,
        system_table: *mut SystemTable,
    ) -> uefi_raw::Status =
    // SAFETY: 'efi_stub_payload_start' points to the valid location in memory.
    unsafe { mem::transmute(efi_payload_start) };

    let status = efi_entry(EFI_IMAGE_HANDLE, system_table);
    error!("EFI payload returned: {:?}", status);
    RebootReason::InvalidPayload
}

pub fn locate_and_execute_efi_payload(
    payload_start: usize,
    payload_size: usize,
    system_table: *mut SystemTable,
) -> RebootReason {
    let efi_payload_start = linux::locate_linux_efi_entrypoint(payload_start, payload_size);

    if efi_payload_start == 0 {
        return RebootReason::InvalidPayload;
    }

    execute_efi_payload(efi_payload_start, system_table)
}
