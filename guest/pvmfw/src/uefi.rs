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
use crate::uefi::linux::LINUX_EFI_LOADED_IMAGE_FIXED_GUID;
use crate::uefi::linux::RT_PROPERTIES_TABLE_GUID;
use crate::uefi::loaded_image::LOADED_IMAGE_PROTOCOL_GUID;
use crate::uefi::runtime_services::RtPropertiesTable;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem;
use core::ptr::{addr_of_mut, null, null_mut};
use core::sync::atomic::{AtomicUsize, Ordering};
use log::info;
use runtime_services::init_rt_properties_table;
use spin::mutex::SpinMutex;
use uefi_raw::protocol::console::SimpleTextOutputProtocol;
use uefi_raw::protocol::loaded_image::LoadedImageProtocol;
use uefi_raw::table::boot::{BootServices, MemoryAttribute, MemoryDescriptor, MemoryType};
use uefi_raw::table::configuration::ConfigurationTable;
use uefi_raw::table::runtime::RuntimeServices;
use uefi_raw::table::system::SystemTable;
use uefi_raw::table::{Header, Revision};
use uefi_raw::Guid;
use uefi_raw::Status;
use vmbase::arch::disable_wxn;
use vmbase::memory::{map_data_noflush, SIZE_4KB};

const EFI_2_100_SYSTEM_TABLE_REVISION: u32 = (2 << 16) | (100);
const EFI_SPECIFICATION_REVISION: u32 = EFI_2_100_SYSTEM_TABLE_REVISION;

static EFI_LOADER: SpinMutex<EfiLoader> = SpinMutex::new(EfiLoader::new());

/// Represents UEFI structures used for booting the Linux kernel through EFI stub.
pub struct EfiLoader {
    pub system_table: SystemTable,
    boot_services: BootServices,
    runtime_services: RuntimeServices,
    simple_text_output_protocol: SimpleTextOutputProtocol,
    loaded_image_protocol: LoadedImageProtocol,
    firmware_vendor: [u16; 6],
    rt_properties_table: RtPropertiesTable,
    configuration_table: Vec<ConfigurationTable>,
    kernel_address: usize,
    kernel_size: usize,
    initrd_address: usize,
    initrd_size: usize,
    fdt_address: usize,
    fdt_size: usize,
}

static ALLOCATED_PAGES: AtomicUsize = AtomicUsize::new(0);

impl EfiLoader {
    pub const EFI_IMAGE_HANDLE: usize = 0x19ef_781b;
    // TODO(nikolinailic): Check whether we can use this for the desc size.
    pub const MEM_MAP_DESC_SIZE: usize = mem::size_of::<MemoryDescriptor>();
    pub const MEM_MAP_DESC_VERSION: u32 = 1;
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
        let kernel_address = 0 as usize;
        let kernel_size = 0 as usize;
        let initrd_address = 0 as usize;
        let initrd_size = 0 as usize;
        let fdt_address = 0 as usize;
        let fdt_size = 0 as usize;

        Self {
            system_table,
            boot_services,
            runtime_services,
            simple_text_output_protocol,
            loaded_image_protocol,
            firmware_vendor,
            rt_properties_table,
            configuration_table,
            kernel_address,
            kernel_size,
            initrd_address,
            initrd_size,
            fdt_address,
            fdt_size,
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

    pub fn allocate_pool(
        &mut self,
        pool_type: MemoryType,
        size: usize,
        buffer: *mut *mut u8,
    ) -> Result<NonNull<usize>, Status> {
        if pool_type != MemoryType::LOADER_DATA && pool_type != MemoryType::ACPI_RECLAIM {
            log::error!("The Memory Type is not supported! {pool_type:?}");
            return Err(Status::OUT_OF_RESOURCES);
        }

        let Some(_) = NonNull::new(buffer) else {
            return Err(Status::INVALID_PARAMETER);
        };

        let Some(allocated) = vmbase::heap::allocate(size, true) else {
            return Err(Status::OUT_OF_RESOURCES);
        };

        Ok(allocated)
    }

    fn set_alloc_range(
        &mut self,
        kernel_address: usize,
        kernel_size: usize,
        initrd_address: usize,
        initrd_size: usize,
        fdt_address: usize,
        fdt_size: usize,
    ) {
        self.kernel_address = kernel_address;
        self.kernel_size = kernel_size;
        self.initrd_address = initrd_address;
        self.initrd_size = initrd_size;
        self.fdt_address = fdt_address;
        self.fdt_size = fdt_size;
    }

    pub fn get_memory_map(&mut self) -> Vec<MemoryDescriptor> {
        let kernel_phys_start = self.kernel_address as u64;
        let kernel_page_count = (self.kernel_size).div_ceil(SIZE_4KB.try_into().unwrap()) as u64;

        let initrd_phys_start = self.initrd_address as u64;
        let initrd_page_count = (self.initrd_size).div_ceil(SIZE_4KB.try_into().unwrap()) as u64;

        let fdt_phys_start = self.fdt_address as u64;
        let fdt_page_count = (self.fdt_size).div_ceil(SIZE_4KB.try_into().unwrap()) as u64;

        let free_mem_start = initrd_phys_start + self.initrd_size as u64;
        let free_mem_total = fdt_phys_start - free_mem_start;
        let free_mem_page_count = free_mem_total.div_ceil(SIZE_4KB.try_into().unwrap()) as u64;

        info!("----------------------------------------------- {:?}", free_mem_page_count);

        // Memory map layout:
        // - 1st entry - KERNEL.
        // - 2nd entry - INITRD.
        // - 3rd entry - FDT.
        // - 4th entry - pvmfw.
        // - 5th entry - FREE MEMORY.
        let mut memory_map = Vec::new();
        memory_map.push(MemoryDescriptor {
            ty: MemoryType::LOADER_DATA,
            phys_start: kernel_phys_start,
            virt_start: kernel_phys_start,
            // page_count: 12288,
            page_count: kernel_page_count,
            att: MemoryAttribute::WRITE_BACK,
        });
        memory_map.push(MemoryDescriptor {
            ty: MemoryType::LOADER_DATA,
            phys_start: initrd_phys_start,
            virt_start: initrd_phys_start,
            page_count: initrd_page_count,
            att: MemoryAttribute::WRITE_BACK,
        });
        memory_map.push(MemoryDescriptor {
            ty: MemoryType::LOADER_DATA,
            phys_start: fdt_phys_start,
            virt_start: fdt_phys_start,
            page_count: fdt_page_count,
            att: MemoryAttribute::WRITE_BACK,
        });
        memory_map.push(MemoryDescriptor {
            ty: MemoryType::CONVENTIONAL,
            phys_start: free_mem_start,
            virt_start: free_mem_start,
            page_count: free_mem_page_count,
            att: MemoryAttribute::WRITE_BACK,
        });

        let allocated_pages = self.get_allocated_pages();
        if allocated_pages > 0 {
            let allocated_size = u64::try_from(allocated_pages * SIZE_4KB).unwrap();
            let allocated_pages = allocated_pages.try_into().unwrap();
            let last = memory_map.last_mut().unwrap();
            last.page_count = allocated_pages;
            last.ty = MemoryType::BOOT_SERVICES_DATA;
            memory_map.push(MemoryDescriptor {
                ty: MemoryType::CONVENTIONAL,
                phys_start: free_mem_start + allocated_size,
                virt_start: free_mem_start + allocated_size,
                page_count: free_mem_page_count - allocated_pages,
                att: MemoryAttribute::WRITE_BACK,
            })
        }

        for descriptor in memory_map.iter() {
            info!(
                "Type: {:?}, Physical Start: {:#x}, Virtual Start: {:#x}, Page Count: {}, Attributes: {:?}",
                descriptor.ty,
                descriptor.phys_start,
                descriptor.virt_start,
                descriptor.page_count,
                descriptor.att,
            );
        }

        memory_map
    }

    pub fn add_allocated_pages(&mut self, n: usize) -> Option<usize> {
        let allocated_pages = ALLOCATED_PAGES.load(Ordering::Relaxed) + n;
        // let free_mem_page_count = FREE_COUNT as usize;
        let initrd_phys_start = self.initrd_address as u64;
        let fdt_phys_start = self.fdt_address as u64;
        let free_mem_start = initrd_phys_start + self.initrd_size as u64;
        let free_mem_total = fdt_phys_start - free_mem_start;
        let free_mem_page_count = free_mem_total.div_ceil(SIZE_4KB.try_into().unwrap()) as u64;

        if allocated_pages > free_mem_page_count as usize {
            return None;
        }

        ALLOCATED_PAGES.fetch_add(n, Ordering::Relaxed);
        Some(allocated_pages)
    }

    fn get_allocated_pages(&mut self) -> usize {
        ALLOCATED_PAGES.load(Ordering::Relaxed)
    }

    fn patch_pointers(&mut self, payload: &[u8]) {
        self.system_table.boot_services = addr_of_mut!(self.boot_services) as _;
        self.system_table.runtime_services = addr_of_mut!(self.runtime_services) as _;
        self.system_table.firmware_vendor = addr_of_mut!(self.firmware_vendor) as _;
        self.system_table.stdout = addr_of_mut!(self.simple_text_output_protocol) as _;
        self.system_table.stderr = addr_of_mut!(self.simple_text_output_protocol) as _;

        self.loaded_image_protocol.system_table = &mut self.system_table as *mut _;

        self.set_loaded_image_protocol(payload);
    }

    pub fn get_protocol(&mut self, guid: Guid) -> Option<*mut c_void> {
        match guid {
            LOADED_IMAGE_PROTOCOL_GUID => Some(addr_of_mut!(self.loaded_image_protocol) as _),
            LINUX_EFI_LOADED_IMAGE_FIXED_GUID => Some(null_mut()),
            _ => None,
        }
    }

    fn push_to_config_table(&mut self, vendor_guid: Guid, vendor_table: *mut c_void) {
        let configuration_table_entry = ConfigurationTable { vendor_guid, vendor_table };

        self.configuration_table.push(configuration_table_entry);
        self.system_table.configuration_table = self.configuration_table.as_mut_ptr();
        self.system_table.number_of_configuration_table_entries = self.configuration_table.len();
    }

    pub fn install_configuration_table(
        &mut self,
        guid_entry: *const Guid,
        table_ptr: *const c_void,
    ) -> Status {
        if !non_null_and_aligned_const(guid_entry) {
            return Status::INVALID_PARAMETER;
        }

        // SAFETY: 'guid_entry' is not null and is aligned.
        let proto_guid = unsafe { *guid_entry };

        for (index, entry) in self.configuration_table.iter().enumerate() {
            if entry.vendor_guid == proto_guid && table_ptr.is_null() {
                // Remove this entry.
                self.configuration_table.remove(index);
                return Status::SUCCESS;
            } else if entry.vendor_guid == proto_guid && !table_ptr.is_null() {
                // Update this entry.
                self.configuration_table[index].vendor_table = table_ptr as _;
                return Status::SUCCESS;
            }
        }

        if !table_ptr.is_null() {
            // Add new entry.
            self.push_to_config_table(proto_guid, table_ptr as _);
            Status::SUCCESS
        } else {
            Status::NOT_FOUND
        }
    }
}

// SAFETY: Send trait indicates whether a type is safe to transfer across threads. We are working
// with single-threaded system so this operation has no meaningful impact on Rust.
unsafe impl Send for EfiLoader {}

// Initialize parameters passed to the EFI stub by the EFI loader.
pub fn init_efi(payload: &[u8], fdt: &mut [u8]) {
    let mut efi_loader = EFI_LOADER.lock();

    // let initrd = slices.ramdisk.unwrap();
    // let alloc_region_start = (u64::try_from(initrd.as_ptr() as usize).unwrap()
    //     + u64::try_from(initrd.len()).unwrap())
    // .next_multiple_of(SIZE_4KB.try_into().unwrap()) as usize;

    // let alloc_region_end = fdt;
    // let alloc_region_size =
    //     alloc_region_end.checked_sub(alloc_region_start).unwrap().try_into().unwrap();
    // // TODO(nikolinailic): Add map error.
    // let _ = map_data_noflush(alloc_region_start, alloc_region_size);

    efi_loader.set_alloc_range(
        payload_start,
        payload_size,
        initrd_address,
        initrd_size,
        fdt_address,
        fdt_size,
    );

    let alloc_region_start = (initrd_address + initrd_size) as u64;
    let alloc_region_end = fdt_address;
    let alloc_region_size = alloc_region_end
        .checked_sub(alloc_region_start.try_into().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    // TODO(nikolinailic): Add map error.
    let _ = map_data_noflush(alloc_region_start.try_into().unwrap(), alloc_region_size);

    info!("Alloc region start: {:#x}", alloc_region_start);

    let rt_properties_table_ptr = efi_loader.get_rt_properties_table_ptr();
    efi_loader.push_to_config_table(RT_PROPERTIES_TABLE_GUID, rt_properties_table_ptr);
    efi_loader.push_to_config_table(DEVICE_TREE_GUID, fdt.as_ptr() as *mut c_void);

    efi_loader.patch_pointers(payload);
}

pub type EfiEntrypoint = extern "efiapi" fn(usize, *mut SystemTable) -> uefi_raw::Status;

pub fn execute_efi_payload(entrypoint: EfiEntrypoint) -> Status {
    // TODO(ptosi): Parse EFI header and map executable & data sections separately.
    // Until then, allow the EFI payload to run from R/W data mappings.
    // SAFETY: Disabling SCTLR_EL1.WXN has no visible effect on Rust.
    unsafe { disable_wxn() };

    let system_table = EFI_LOADER.lock().get_system_table_ptr();
    entrypoint(EfiLoader::EFI_IMAGE_HANDLE, system_table)
}

pub fn non_null_and_aligned_const<T>(ptr: *const T) -> bool {
    !ptr.is_null() & ptr.is_aligned()
}

pub fn non_null_and_aligned_mut<T>(ptr: *mut T) -> bool {
    !ptr.is_null() & ptr.is_aligned()
}
