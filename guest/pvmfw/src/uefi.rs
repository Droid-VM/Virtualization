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
use crate::uefi::linux::RT_PROPERTIES_TABLE_GUID;
use crate::uefi::loaded_image::LOADED_IMAGE_PROTOCOL_GUID;
use crate::uefi::runtime_services::RtPropertiesTable;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem;
use core::num::NonZeroUsize;
use core::ptr::{addr_of_mut, null, null_mut, NonNull};
use runtime_services::init_rt_properties_table;
use spin::mutex::SpinMutex;
use uefi_raw::protocol::console::SimpleTextOutputProtocol;
use uefi_raw::protocol::loaded_image::LoadedImageProtocol;
use uefi_raw::table::boot::{BootServices, MemoryType};
use uefi_raw::table::configuration::ConfigurationTable;
use uefi_raw::table::runtime::RuntimeServices;
use uefi_raw::table::system::SystemTable;
use uefi_raw::table::{Header, Revision};
use uefi_raw::{Guid, Handle, Status};
use vmbase::arch::disable_wxn;

const EFI_2_100_SYSTEM_TABLE_REVISION: u32 = (2 << 16) | (100);
const EFI_SPECIFICATION_REVISION: u32 = EFI_2_100_SYSTEM_TABLE_REVISION;

static EFI_LOADER: SpinMutex<EfiLoader> = SpinMutex::new(EfiLoader::new());

/// Represents the UEFI structures used for booting EFI payloads.
struct EfiLoader {
    pub system_table: SystemTable,
    boot_services: BootServices,
    runtime_services: RuntimeServices,
    simple_text_output_protocol: SimpleTextOutputProtocol,
    loaded_image_protocol: LoadedImageProtocol,
    firmware_vendor: [u16; 6],
    rt_properties_table: RtPropertiesTable,
    configuration_table: Vec<ConfigurationTable>,
}

impl EfiLoader {
    const EFI_IMAGE_HANDLE: Handle = 0x19ef_781b as _;
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

    fn patch_pointers(&mut self, payload: &[u8]) {
        self.system_table.boot_services = addr_of_mut!(self.boot_services).cast();
        self.system_table.runtime_services = addr_of_mut!(self.runtime_services).cast();
        self.system_table.firmware_vendor = addr_of_mut!(self.firmware_vendor).cast();
        self.system_table.stdout = addr_of_mut!(self.simple_text_output_protocol).cast();
        self.system_table.stderr = addr_of_mut!(self.simple_text_output_protocol).cast();

        let rt_properties_table = addr_of_mut!(self.rt_properties_table).cast();
        let _ = self.insert_config_table(RT_PROPERTIES_TABLE_GUID, rt_properties_table);

        self.loaded_image_protocol.system_table = addr_of_mut!(self.system_table).cast();
        self.set_loaded_image_protocol(payload);
    }

    fn set_loaded_image_protocol(&mut self, payload: &[u8]) {
        self.loaded_image_protocol.image_base = payload.as_ptr().cast();
        let image_size = payload.len().try_into().unwrap();
        self.loaded_image_protocol.image_size = image_size;
    }

    fn uses_image_handle(&self, handle: Handle) -> bool {
        handle == Self::EFI_IMAGE_HANDLE
    }

    fn get_system_table_ptr(&mut self) -> *mut SystemTable {
        addr_of_mut!(self.system_table)
    }

    fn sync_system_config_table(&mut self) {
        self.system_table.configuration_table = self.configuration_table.as_mut_ptr();
        self.system_table.number_of_configuration_table_entries = self.configuration_table.len();
    }

    fn insert_config_table(&mut self, guid: Guid, table: *mut c_void) -> Option<*mut c_void> {
        if let Some(i) = self.configuration_table.iter().position(|e| e.vendor_guid == guid) {
            let prev = self.configuration_table[i].vendor_table;
            self.configuration_table[i].vendor_table = table as _;
            Some(prev)
        } else {
            let entry = ConfigurationTable { vendor_guid: guid, vendor_table: table };
            self.configuration_table.push(entry);
            self.sync_system_config_table();
            None
        }
    }

    fn remove_config_table(&mut self, guid: Guid) -> Option<*mut c_void> {
        let i = self.configuration_table.iter().position(|e| e.vendor_guid == guid)?;
        let entry = self.configuration_table.remove(i).vendor_table;
        self.sync_system_config_table();
        Some(entry)
    }

    pub fn allocate_pool(
        &mut self,
        memory_type: MemoryType,
        size: NonZeroUsize,
    ) -> Option<NonNull<u8>> {
        match memory_type {
            MemoryType::LOADER_DATA | MemoryType::ACPI_RECLAIM => {
                vmbase::heap::allocate(size.get(), true).map(|p| p.cast())
            }
            _ => None,
        }
    }

    pub fn get_protocol(&mut self, guid: Guid) -> Option<*mut c_void> {
        match guid {
            LOADED_IMAGE_PROTOCOL_GUID => Some(addr_of_mut!(self.loaded_image_protocol).cast()),
            _ => None,
        }
    }

    pub fn get_all_handles(&self) -> Vec<Handle> {
        vec![Self::EFI_IMAGE_HANDLE]
    }

    pub fn get_protocol_handles(&mut self, guid: Guid) -> Vec<Handle> {
        if self.get_protocol(guid).is_some() {
            vec![Self::EFI_IMAGE_HANDLE]
        } else {
            vec![]
        }
    }
}

// SAFETY: `Send` is not relevant as pvmfw is single-threaded.
unsafe impl Send for EfiLoader {}

pub fn init_efi(payload: &[u8]) {
    let mut efi_loader = EFI_LOADER.lock();
    efi_loader.patch_pointers(payload);
}

pub type EfiEntrypoint = extern "efiapi" fn(Handle, *mut SystemTable) -> uefi_raw::Status;

pub fn execute_efi_payload(entrypoint: EfiEntrypoint) -> Status {
    // TODO(ptosi): Parse EFI header and map executable & data sections separately.
    // Until then, allow the EFI payload to run from R/W data mappings.
    disable_wxn();

    let system_table = EFI_LOADER.lock().get_system_table_ptr();
    entrypoint(EfiLoader::EFI_IMAGE_HANDLE, system_table)
}

pub fn non_null_and_aligned_const<T>(ptr: *const T) -> bool {
    !ptr.is_null() & ptr.is_aligned()
}

pub fn non_null_and_aligned_mut<T>(ptr: *mut T) -> Option<NonNull<T>> {
    let non_null = NonNull::new(ptr)?;
    if ptr.is_aligned() {
        Some(non_null)
    } else {
        None
    }
}
