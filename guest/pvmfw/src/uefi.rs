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

//! EFI stub support in pvmfw.

#![allow(unused_variables)]

use crate::entry::EFI_IMAGE_HANDLE;
use crate::entry::END_OF_MEMORY;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem;
use core::ptr::{copy, null, null_mut, write_bytes, NonNull};
use core::sync::atomic::{AtomicUsize, Ordering};
use log::{error, info};
use spin::mutex::SpinMutex;
use uefi_raw::capsule::CapsuleHeader;
use uefi_raw::protocol::console::SimpleTextOutputProtocol;
use uefi_raw::protocol::device_path::DevicePathProtocol;
use uefi_raw::protocol::loaded_image::LoadedImageProtocol;
use uefi_raw::table::boot::{
    BootServices, EventNotifyFn, EventType, InterfaceType, MemoryAttribute, MemoryDescriptor,
    MemoryType, OpenProtocolInformationEntry, Tpl,
};
use uefi_raw::table::configuration::ConfigurationTable;
use uefi_raw::table::runtime::{ResetType, RuntimeServices, TimeCapabilities, VariableAttributes};
use uefi_raw::table::system::SystemTable;
use uefi_raw::table::{Header, Revision};
use uefi_raw::time::Time;
use uefi_raw::{guid, Char16, Event, Guid, Handle, PhysicalAddress, Status};
use vmbase::memory::SIZE_4KB;

const LINUX_EFI_LOADED_IMAGE_FIXED_GUID: Guid = guid!("f5a37b6d-3344-42a5-b6bb-978648c1890a");
const LOADED_IMAGE_PROTOCOL_GUID: Guid = guid!("5b1b31a1-9562-11d2-8e3f-00a0c969723b");
const RT_PROPERTIES_TABLE_GUID: Guid = guid!("eb66918a-7eef-402a-842e-931d21c38ae9");
const DEVICE_TREE_GUID: Guid = guid!("b1b621d5-f19c-41a5-830b-d9152c69aae0");

const ALLOC_POINTER: usize = 0x8ce46000;

const KERNEL_START: u64 = 0x80000000;
const KERNEL_COUNT: u64 = 0x22a3;
const INITRD_START: u64 = 0x83000000;
const INITRD_COUNT: u64 = 0x206;
const FREE_START: u64 = 0x83206000;
const FREE_COUNT: u64 = 0x9c40;

static MEM_MAP_KEY_COUNTER: AtomicUsize = AtomicUsize::new(0);

static mut ALLOCATED_PAGES: usize = 0;

pub static EFI_LOADER: SpinMutex<EfiLoader> = SpinMutex::new(EfiLoader::new());

/// Logs the invocation of all functions by the EFI stub with their name and arguments at the trace
/// level.
macro_rules! log_call {
    ($fn:ident $(,$arg:ident)*) => {{
        log::trace!(
            concat!(
                stringify!($fn),
                "(",
                $(stringify!($arg), "={:?}, ", )*
                ")",
            ),
            $($arg,)*
        );
    }}
}
/// Represents UEFI structures used for booting the Linux kernel through EFI stub.
///
/// - SystemTable: The main structure that contains pointers to the runtime and boot services
///   tables.
/// - BootServices: Contains pointers to all boot services.
/// - RuntimeServices: Contains pointers to all runtime services.
/// - SimpleTextOutputProtocol: Controls text-based output devices.
/// - LoadedImageProtocol: Provides information about the loaded image.
/// - RtPropertiesTable: Defines which runtime services the system provides. Published by a platform
///   if it does not support all runtime services.
/// - ConfigurationTable: Contains a set of GUID/pointer pairs, where the pointer represents the
///   table associated with the GUID.
///
/// This should be stored in a static SpinMutex variable to avoid unsafe code blocks.
pub struct EfiLoader {
    pub system_table: SystemTable,
    boot_services: BootServices,
    runtime_services: RuntimeServices,
    simple_text_output_protocol: SimpleTextOutputProtocol,
    pub loaded_image_protocol: LoadedImageProtocol,
    rt_properties_table: RtPropertiesTable,
    configuration_table: Vec<ConfigurationTable>,
}

impl EfiLoader {
    pub const fn new() -> Self {
        let system_table = SystemTable {
            header: Header {
                signature: SystemTable::SIGNATURE,
                revision: Revision(0),
                size: mem::size_of::<SystemTable>() as _,
                crc: 0,
                reserved: 0,
            },

            firmware_vendor: null_mut(),
            firmware_revision: 0,

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

        let boot_services = BootServices {
            header: Header {
                signature: 0,
                revision: Revision(0),
                size: mem::size_of::<BootServices>() as _,
                crc: 0,
                reserved: 0,
            },

            raise_tpl,
            restore_tpl,

            allocate_pages,
            free_pages,
            get_memory_map,
            allocate_pool,
            free_pool,

            create_event,
            set_timer,
            wait_for_event,
            signal_event,
            close_event,
            check_event,

            install_protocol_interface,
            reinstall_protocol_interface,
            uninstall_protocol_interface,
            handle_protocol,
            reserved: null_mut(),
            register_protocol_notify,
            locate_handle,
            locate_device_path,
            install_configuration_table,

            load_image,
            start_image,
            exit,
            unload_image,
            exit_boot_services,

            get_next_monotonic_count,
            stall,
            set_watchdog_timer,

            connect_controller,
            disconnect_controller,

            open_protocol,
            close_protocol,
            open_protocol_information,

            protocols_per_handle,
            locate_handle_buffer,
            locate_protocol,

            install_multiple_protocol_interfaces,
            uninstall_multiple_protocol_interfaces,

            calculate_crc32,

            copy_mem,
            set_mem,

            create_event_ex,
        };

        let runtime_services = RuntimeServices {
            header: Header {
                signature: 0,
                revision: Revision(0),
                size: mem::size_of::<RuntimeServices>() as _,
                crc: 0,
                reserved: 0,
            },
            get_time,
            set_time,
            get_wakeup_time,
            set_wakeup_time,
            set_virtual_address_map,
            convert_pointer,
            get_variable,
            get_next_variable_name,
            set_variable,
            get_next_high_monotonic_count,
            reset_system,

            update_capsule,
            query_capsule_capabilities,

            query_variable_info,
        };

        let simple_text_output_protocol = SimpleTextOutputProtocol {
            reset,
            output_string,
            test_string,
            query_mode,
            set_mode,
            set_attribute,
            clear_screen,
            set_cursor_position,
            enable_cursor,
            mode: null_mut(),
        };

        let loaded_image_protocol = LoadedImageProtocol {
            revision: 0,
            parent_handle: null_mut(),
            system_table: null_mut(),

            device_handle: null_mut(),
            file_path: null(),

            reserved: null(),

            load_options_size: 0,
            load_options: null(),

            image_base: null(),
            image_size: 0,
            image_code_type: MemoryType::LOADER_DATA,
            image_data_type: MemoryType::LOADER_DATA,
            unload: Some(unload),
        };

        let rt_properties_table =
            RtPropertiesTable { version: 0, length: 8, runtime_services_supported: 0 };

        let configuration_table = Vec::new();

        Self {
            system_table,
            boot_services,
            runtime_services,
            simple_text_output_protocol,
            loaded_image_protocol,
            rt_properties_table,
            configuration_table,
        }
    }

    pub fn patch_pointers(&mut self) {
        self.system_table.boot_services = &mut self.boot_services as *mut _;
        self.system_table.runtime_services = &mut self.runtime_services as *mut _;
        self.system_table.stdout = &mut self.simple_text_output_protocol as *mut _;
        self.system_table.stderr = &mut self.simple_text_output_protocol as *mut _;
        self.loaded_image_protocol.system_table = &mut self.system_table as *mut _;
    }
}

// SAFETY: TODO(nikolinailic).
unsafe impl Send for EfiLoader {}

#[allow(dead_code)]
pub struct RtPropertiesTable {
    pub version: u16,
    pub length: u16,
    pub runtime_services_supported: u32,
}

// Initialize parameters passed to the EFI stub by the EFI loader.
pub fn init_efi() {
    let rt_properties_table_ptr = get_rt_properties_table_ptr();
    push_to_config_table(RT_PROPERTIES_TABLE_GUID, rt_properties_table_ptr);
    push_to_config_table(DEVICE_TREE_GUID, 0x8fe00000 as *mut c_void);

    EFI_LOADER.lock().patch_pointers();
}

/// Task Priority functions - boot services.
unsafe extern "efiapi" fn raise_tpl(new_tpl: Tpl) -> Tpl {
    log_call!(RaiseTpl, new_tpl);
    Tpl(0)
}
unsafe extern "efiapi" fn restore_tpl(new_tpl: Tpl) {
    log_call!(RestoreTpl, new_tpl);
}

/// Memory allocation functions - boot services.
unsafe extern "efiapi" fn allocate_pages(
    alloc_ty: u32,
    mem_ty: MemoryType,
    count: usize,
    addr: *mut PhysicalAddress,
) -> Status {
    if addr.is_null() || mem_ty == MemoryType::PERSISTENT_MEMORY || mem_ty == MemoryType::UNACCEPTED
    {
        return Status::INVALID_PARAMETER;
    }
    // SAFETY: This is safe as raw pointer 'addr' is not null.
    let addr_value = unsafe { *addr } as *const u8;
    log_call!(AllocatePages, alloc_ty, mem_ty, count, addr, addr_value);

    let offset = get_allocated_pages() * SIZE_4KB;
    let start = (ALLOC_POINTER + offset).try_into().unwrap();
    let _ = add_allocated_pages(count).unwrap();
    // SAFETY: This is safe as raw pointer 'addr' is not null.
    unsafe { addr.write(start) };

    info!("Allocated at {start:#x}");
    increment_mem_map_counter();

    Status::SUCCESS
}
unsafe extern "efiapi" fn free_pages(addr: PhysicalAddress, pages: usize) -> Status {
    log_call!(FreePages, addr, pages);
    Status::SUCCESS
}
unsafe extern "efiapi" fn get_memory_map(
    size: *mut usize,
    map: *mut MemoryDescriptor,
    key: *mut usize,
    desc_size: *mut usize,
    desc_version: *mut u32,
) -> Status {
    log_call!(GetMemoryMap, size, map, key, desc_size, desc_version);
    if size.is_null() || key.is_null() || desc_size.is_null() || desc_version.is_null() {
        error!("GetMemoryMap({size:?}, {map:?}, {key:?}, {desc_size:?}, {desc_version:?})");
        return Status::INVALID_PARAMETER;
    }
    let kernel_phys_start = KERNEL_START;
    let kernel_page_count = KERNEL_COUNT;
    let initrd_phys_start = INITRD_START;
    let initrd_page_count = INITRD_COUNT;
    let free_mem_start: u64 = FREE_START;
    let free_mem_page_count = FREE_COUNT;

    let free_mem_total = END_OF_MEMORY.load(Ordering::Relaxed) - free_mem_start as usize;
    let free_mem_page_count_new = free_mem_total.div_ceil(SIZE_4KB) as u64;
    info!("----------------------------------------------- {:?}", free_mem_page_count_new);
    let mut memory_map = Vec::new();
    memory_map.push(MemoryDescriptor {
        ty: MemoryType::LOADER_DATA,
        phys_start: kernel_phys_start,
        virt_start: kernel_page_count,
        page_count: 12288,
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
        ty: MemoryType::CONVENTIONAL,
        phys_start: free_mem_start,
        virt_start: free_mem_start,
        page_count: free_mem_page_count_new,
        att: MemoryAttribute::WRITE_BACK,
    });

    let allocated_pages = get_allocated_pages();
    if allocated_pages > 0 {
        let allocated_size = u64::try_from(allocated_pages * SIZE_4KB).unwrap();
        let allocated_pages = allocated_pages.try_into().unwrap();
        let last = memory_map.last_mut().unwrap();
        last.page_count = allocated_pages;
        last.ty = MemoryType::LOADER_DATA;
        memory_map.push(MemoryDescriptor {
            ty: MemoryType::CONVENTIONAL,
            phys_start: free_mem_start + allocated_size,
            virt_start: free_mem_start + allocated_size,
            page_count: free_mem_page_count_new - allocated_pages,
            att: MemoryAttribute::WRITE_BACK,
        })
    }
    let memory_map_size = memory_map.len() * mem::size_of::<MemoryDescriptor>();
    // if *size < memory_map_size { return }

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

    let mut status = Status::SUCCESS;
    if !map.is_null() {
        // SAFETY: TODO(nikolinailic)
        unsafe { copy(memory_map.as_ptr(), map, memory_map.len()) };
    } else {
        status = Status::BUFFER_TOO_SMALL;
    }

    // SAFETY: TODO(nikolinailic)
    unsafe {
        *size = memory_map_size;
        *key = MEM_MAP_KEY_COUNTER.load(Ordering::Relaxed);
        *desc_size = mem::size_of::<MemoryDescriptor>();
        *desc_version = 1;
    }
    status
}
unsafe extern "efiapi" fn allocate_pool(
    pool_type: MemoryType,
    size: usize,
    buffer: *mut *mut u8,
) -> Status {
    log_call!(AllocatePool, pool_type, size, buffer);
    if size == 0 || buffer.is_null() {
        return Status::INVALID_PARAMETER;
    }

    if pool_type != MemoryType::LOADER_DATA && pool_type != MemoryType::ACPI_RECLAIM {
        log::error!("The Memory Type is not supported! {pool_type:?}");
        return Status::OUT_OF_RESOURCES;
    }

    let Some(buffer) = NonNull::new(buffer) else {
        return Status::INVALID_PARAMETER;
    };

    let Some(allocated) = vmbase::heap::allocate(size, true) else {
        return Status::OUT_OF_RESOURCES;
    };

    // Safety: This is safe as raw pointer 'buffer' is not null.
    unsafe {
        buffer.write(allocated.as_ptr().cast());
    }

    Status::SUCCESS
}
unsafe extern "efiapi" fn free_pool(buffer: *mut u8) -> Status {
    log_call!(FreePool, buffer);
    let void_buffer: *mut c_void = buffer as *mut c_void;
    let Some(ptr) = NonNull::new(void_buffer) else { return Status::INVALID_PARAMETER };
    vmbase::heap::deallocate(ptr);
    Status::SUCCESS
}

/// Event & timer functions - boot services.
unsafe extern "efiapi" fn create_event(
    ty: EventType,
    notify_tpl: Tpl,
    notify_func: Option<EventNotifyFn>,
    notify_ctx: *mut c_void,
    out_event: *mut Event,
) -> Status {
    log_call!(CreateEvent, ty, notify_tpl, notify_func, notify_ctx, out_event);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_timer(event: Event, ty: u32, trigger_time: u64) -> Status {
    log_call!(SetTimer, event, ty, trigger_time);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn wait_for_event(
    number_of_events: usize,
    events: *mut Event,
    out_index: *mut usize,
) -> Status {
    log_call!(WaitForEvent, number_of_events, events, out_index);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn signal_event(event: Event) -> Status {
    log_call!(SignalEvent, event);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn close_event(event: Event) -> Status {
    log_call!(CloseEvent, event);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn check_event(event: Event) -> Status {
    log_call!(CheckEvent, event);
    Status::UNSUPPORTED
}

// LoadedImageProtocol function - not supported.
unsafe extern "efiapi" fn unload(image_handle: Handle) -> Status {
    log_call!(LoadedImageProtocol_Unload, image_handle);
    Status::UNSUPPORTED
}

/// Protocol handler functions - boot services.
unsafe extern "efiapi" fn install_protocol_interface(
    handle: *mut Handle,
    guid: *const Guid,
    interface_type: InterfaceType,
    interface: *const c_void,
) -> Status {
    log_call!(InstallProtocolInterface, handle, guid, interface_type, interface);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn reinstall_protocol_interface(
    handle: Handle,
    protocol: *const Guid,
    old_interface: *const c_void,
    new_interface: *const c_void,
) -> Status {
    log_call!(ReinstallProtocolInterface, handle, protocol, old_interface, new_interface);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn uninstall_protocol_interface(
    handle: Handle,
    protocol: *const Guid,
    interface: *const c_void,
) -> Status {
    log_call!(UninstallProtocolInterface, handle, protocol, interface);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn handle_protocol(
    handle: Handle,
    proto: *const Guid,
    out_proto: *mut *mut c_void,
) -> Status {
    if handle.is_null() || proto.is_null() || out_proto.is_null() {
        return Status::INVALID_PARAMETER;
    }
    // SAFETY: This is safe as raw pointer 'proto' is not null.
    let proto_guid = unsafe { *proto };
    log_call!(HandleProtocol, handle, proto, proto_guid, out_proto);

    // Check whether the received image handle matches the image handle pvmfw passed via
    // 'jump_to_payload_with_efi_stub' function before entering the EFI stub.
    let ptr = EFI_IMAGE_HANDLE as *mut c_void;
    if handle != ptr {
        return Status::UNSUPPORTED;
    }

    set_protocol_interface_pointer(proto_guid, out_proto)
}
unsafe extern "efiapi" fn register_protocol_notify(
    protocol: *const Guid,
    event: Event,
    registration: *mut *const c_void,
) -> Status {
    log_call!(RegisterProtocolNotify, protocol, event, registration);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn locate_handle(
    search_ty: i32,
    proto: *const Guid,
    key: *const c_void,
    buf_sz: *mut usize,
    buf: *mut Handle,
) -> Status {
    if search_ty != 0 && search_ty != 1 && search_ty != 2 {
        return Status::INVALID_PARAMETER;
    }
    if (search_ty == 1 && key.is_null())
        || (search_ty == 2 && proto.is_null())
        || buf_sz.is_null()
        || proto.is_null()
        || buf.is_null()
    {
        return Status::INVALID_PARAMETER;
    }

    // SAFETY: This is safe as raw pointer 'proto' is not null.
    let proto_guid = unsafe { *proto };
    log_call!(LocateHandle, search_ty, proto, proto_guid, key, buf_sz, buf);

    if search_ty == 2
        && proto_guid != LOADED_IMAGE_PROTOCOL_GUID
        && proto_guid != LINUX_EFI_LOADED_IMAGE_FIXED_GUID
    {
        return Status::NOT_FOUND;
    }

    // SAFETY: This is safe as both raw pointers 'buf_sz' and 'buf' are not null.
    unsafe {
        *buf_sz = 1;
        if !buf.is_null() {
            buf.write(EFI_IMAGE_HANDLE as *mut c_void);
        }
    }

    Status::SUCCESS
}
unsafe extern "efiapi" fn locate_device_path(
    proto: *const Guid,
    device_path: *mut *const DevicePathProtocol,
    out_handle: *mut Handle,
) -> Status {
    log_call!(LocateDevicePath, proto, device_path, out_handle);
    Status::NOT_FOUND
}
unsafe extern "efiapi" fn install_configuration_table(
    guid_entry: *const Guid,
    table_ptr: *const c_void,
) -> Status {
    log_call!(InstallConfigurationTable, guid_entry, table_ptr);

    if guid_entry.is_null() {
        return Status::INVALID_PARAMETER;
    }

    // SAFETY: This is safe as raw pointer 'guid_entry' is not null.
    let proto_guid = unsafe { *guid_entry };

    for (index, entry) in EFI_LOADER.lock().configuration_table.iter().enumerate() {
        if entry.vendor_guid == proto_guid && table_ptr.is_null() {
            // Remove this entry.
            let mut efi_loader = EFI_LOADER.lock();
            efi_loader.configuration_table.remove(index);
            return Status::SUCCESS;
        } else if entry.vendor_guid == proto_guid && !table_ptr.is_null() {
            // Update this entry.
            let mut efi_loader = EFI_LOADER.lock();
            efi_loader.configuration_table[index].vendor_table = table_ptr as _;
            return Status::SUCCESS;
        }
    }

    if !table_ptr.is_null() {
        // Add new entry.
        push_to_config_table(proto_guid, table_ptr as _);
        Status::SUCCESS
    } else {
        Status::NOT_FOUND
    }
}

/// Image service functions - boot services.
unsafe extern "efiapi" fn load_image(
    boot_policy: u8,
    parent_image_handle: Handle,
    device_path: *const DevicePathProtocol,
    source_buffer: *const u8,
    source_size: usize,
    image_handle: *mut Handle,
) -> Status {
    log_call!(
        LoadImage,
        boot_policy,
        parent_image_handle,
        device_path,
        source_buffer,
        source_size,
        image_handle
    );
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn start_image(
    image_handle: Handle,
    exit_data_size: *mut usize,
    exit_data: *mut *mut Char16,
) -> Status {
    log_call!(StartImage, image_handle, exit_data_size, exit_data);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn exit(
    image_handle: Handle,
    exit_status: Status,
    exit_data_size: usize,
    exit_data: *mut Char16,
) -> ! {
    log_call!(Exit, image_handle, exit_status, exit_data_size, exit_data);
    #[allow(clippy::empty_loop)]
    loop {}
}
unsafe extern "efiapi" fn unload_image(image_handle: Handle) -> Status {
    log_call!(UnloadImage, image_handle);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn exit_boot_services(image_handle: Handle, map_key: usize) -> Status {
    log_call!(ExitBootServices, image_handle, map_key);
    if map_key != MEM_MAP_KEY_COUNTER.load(Ordering::Relaxed) {
        return Status::INVALID_PARAMETER;
    }
    Status::SUCCESS
}

/// Misc service functions - boot services.
unsafe extern "efiapi" fn get_next_monotonic_count(count: *mut u64) -> Status {
    log_call!(GetNextMonotonicCount, count);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn stall(microseconds: usize) -> Status {
    log_call!(Stall, microseconds);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_watchdog_timer(
    timeout: usize,
    watchdog_code: u64,
    data_size: usize,
    watchdog_data: *const u16,
) -> Status {
    log_call!(SetWatchdogTimer, timeout, watchdog_code, data_size, watchdog_data);
    Status::UNSUPPORTED
}

// Driver support service functions.
unsafe extern "efiapi" fn connect_controller(
    controller: Handle,
    driver_image: Handle,
    remaining_device_path: *const DevicePathProtocol,
    recursive: bool,
) -> Status {
    log_call!(ConnectController, controller, driver_image, remaining_device_path, recursive);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn disconnect_controller(
    controller: Handle,
    driver_image: Handle,
    child: Handle,
) -> Status {
    log_call!(DisconnectController, controller, driver_image, child);
    Status::UNSUPPORTED
}

/// Protocol open / close service functions - boot services.
unsafe extern "efiapi" fn open_protocol(
    handle: Handle,
    protocol: *const Guid,
    interface: *mut *mut c_void,
    agent_handle: Handle,
    controller_handle: Handle,
    attributes: u32,
) -> Status {
    log_call!(
        OpenProtocol,
        handle,
        protocol,
        interface,
        agent_handle,
        controller_handle,
        attributes
    );
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn close_protocol(
    handle: Handle,
    protocol: *const Guid,
    agent_handle: Handle,
    controller_handle: Handle,
) -> Status {
    log_call!(CloseProtocol, handle, protocol, agent_handle, controller_handle);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn open_protocol_information(
    handle: Handle,
    protocol: *const Guid,
    entry_buffer: *mut *const OpenProtocolInformationEntry,
    entry_count: *mut usize,
) -> Status {
    log_call!(OpenProtocolInformation, handle, protocol, entry_buffer, entry_count);
    Status::UNSUPPORTED
}

/// Library service functions - boot services.
unsafe extern "efiapi" fn protocols_per_handle(
    handle: Handle,
    protocol_buffer: *mut *mut *const Guid,
    protocol_buffer_count: *mut usize,
) -> Status {
    log_call!(ProtocolsPerHandle, handle, protocol_buffer, protocol_buffer_count);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn locate_handle_buffer(
    search_ty: i32,
    proto: *const Guid,
    key: *const c_void,
    no_handles: *mut usize,
    buf: *mut *mut Handle,
) -> Status {
    log_call!(LocateHandleBuffer, search_ty, proto, key, no_handles, buf);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn locate_protocol(
    proto: *const Guid,
    registration: *mut c_void,
    out_proto: *mut *mut c_void,
) -> Status {
    if proto.is_null() || out_proto.is_null() {
        return Status::INVALID_PARAMETER;
    }
    // SAFETY: This is safe as raw pointer 'proto' is not null.
    let proto_guid = unsafe { *proto };
    log_call!(LocateProtocol, proto, proto_guid, registration, out_proto);

    let status = set_protocol_interface_pointer(proto_guid, out_proto);
    // If no match is found, return NOT_FOUND instead of UNSUPPORTED (from the UEFI spec).
    if status == Status::UNSUPPORTED {
        return Status::NOT_FOUND;
    }
    status
}

// TODO(stable(c_variadic))
#[allow(non_upper_case_globals)]
const install_multiple_protocol_interfaces: unsafe extern "C" fn(*mut Handle, ...) -> Status =
    // SAFETY: Variadic and non variadic functions use the same ABI. The function pointer declared
    // as `extern "C"` will work correctly when called from a UEFI target. Support for C-variadics
    // with `efiapi` requires the unstable [`extended_varargs_abi_support`].
    unsafe {
        mem::transmute::<unsafe extern "C" fn(*mut Handle) -> Status, _>(
            install_multiple_protocol_interfaces_non_variadic,
        )
    };

// TODO(stable(c_variadic))
#[allow(non_upper_case_globals)]
const uninstall_multiple_protocol_interfaces: unsafe extern "C" fn(Handle, ...) -> Status =
    // SAFETY: Variadic and non variadic functions use the same ABI. The function pointer declared
    // as `extern "C"` will work correctly when called from a UEFI target. Support for C-variadics
    // with `efiapi` requires the unstable [`extended_varargs_abi_support`].
    unsafe {
        mem::transmute::<unsafe extern "C" fn(Handle) -> Status, _>(
            uninstall_multiple_protocol_interfaces_non_variadic,
        )
    };

unsafe extern "C" fn install_multiple_protocol_interfaces_non_variadic(
    handle: *mut Handle,
) -> Status {
    Status::SUCCESS
}
unsafe extern "C" fn uninstall_multiple_protocol_interfaces_non_variadic(handle: Handle) -> Status {
    Status::SUCCESS
}

/// CRC service functions - boot services.
unsafe extern "efiapi" fn calculate_crc32(
    data: *const c_void,
    data_size: usize,
    crc32: *mut u32,
) -> Status {
    log_call!(CalculateCrc32, data, data_size, crc32);
    Status::UNSUPPORTED
}

/// Misc service functions - boot services.
unsafe extern "efiapi" fn copy_mem(dest: *mut u8, src: *const u8, len: usize) {
    log_call!(CopyMem, dest, src, len);
    if src.is_null() || dest.is_null() {
        error!("CopyMem received NULL: src={src:?}, dest={dest:?}");
        return;
    }
    // SAFETY: This is safe as both raw pointers 'src' and 'dest' are not null.
    unsafe { copy(src, dest, len) };
}
unsafe extern "efiapi" fn set_mem(buffer: *mut u8, len: usize, value: u8) {
    log_call!(SetMem, buffer, len, value);
    if buffer.is_null() {
        error!("set_mem received NULL: buffer={buffer:?}");
        return;
    }
    // SAFETY: This is safe as raw pointer 'buffer' is not null.
    unsafe { write_bytes(buffer, value, len) };
}

/// New event functions (UEFI 2.0 or newer) - boot services.
unsafe extern "efiapi" fn create_event_ex(
    ty: EventType,
    notify_tpl: Tpl,
    notify_fn: Option<EventNotifyFn>,
    notify_ctx: *mut c_void,
    event_group: *mut Guid,
    out_event: *mut Event,
) -> Status {
    log_call!(CreateEventEx, ty, notify_tpl, notify_fn, notify_ctx, event_group, out_event);
    Status::UNSUPPORTED
}

/// Runtime services functions.
unsafe extern "efiapi" fn get_time(time: *mut Time, capabilities: *mut TimeCapabilities) -> Status {
    log_call!(GetTime, time, capabilities);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_time(time: *const Time) -> Status {
    log_call!(SetTime, time);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn get_wakeup_time(
    enabled: *mut u8,
    pending: *mut u8,
    time: *mut Time,
) -> Status {
    log_call!(GetWakeupTime, enabled, pending, time);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_wakeup_time(enable: u8, time: *const Time) -> Status {
    log_call!(SetWakeupTime, enable, time);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_virtual_address_map(
    map_size: usize,
    desc_size: usize,
    desc_version: u32,
    virtual_map: *mut MemoryDescriptor,
) -> Status {
    log_call!(SetVirtualAddressMap, map_size, desc_size, desc_version, virtual_map);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn convert_pointer(
    debug_disposition: usize,
    address: *mut *const c_void,
) -> Status {
    log_call!(ConvertPointer, debug_disposition, address);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn get_variable(
    variable_name: *const Char16,
    vendor_guid: *const Guid,
    attributes: *mut VariableAttributes,
    data_size: *mut usize,
    data: *mut u8,
) -> Status {
    log_call!(GetVariable, variable_name, vendor_guid, attributes, data_size, data);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn get_next_variable_name(
    variable_name_size: *mut usize,
    variable_name: *mut u16,
    vendor_guid: *mut Guid,
) -> Status {
    log_call!(GetNextVariableName, variable_name_size, variable_name, vendor_guid);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_variable(
    variable_name: *const Char16,
    vendor_guid: *const Guid,
    attributes: VariableAttributes,
    data_size: usize,
    data: *const u8,
) -> Status {
    log_call!(SetVariable, variable_name, vendor_guid, attributes, data_size, data);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn get_next_high_monotonic_count(high_count: *mut u32) -> Status {
    log_call!(GetNextHighMonotonicCount, high_count);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn reset_system(
    rt: ResetType,
    status: Status,
    data_size: usize,
    data: *const u8,
) -> ! {
    log_call!(ResetSystem, rt, status, data_size, data);
    #[allow(clippy::empty_loop)]
    loop {}
}
unsafe extern "efiapi" fn update_capsule(
    capsule_header_array: *const *const CapsuleHeader,
    capsule_count: usize,
    scatter_gather_list: PhysicalAddress,
) -> Status {
    log_call!(UpdateCapsule, capsule_header_array, capsule_count, scatter_gather_list);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn query_capsule_capabilities(
    capsule_header_array: *const *const CapsuleHeader,
    capsule_count: usize,
    maximum_capsule_size: *mut u64,
    reset_type: *mut ResetType,
) -> Status {
    log_call!(
        QueryCapsuleCapabilites,
        capsule_header_array,
        capsule_count,
        maximum_capsule_size,
        reset_type
    );
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn query_variable_info(
    attributes: VariableAttributes,
    maximum_variable_storage_size: *mut u64,
    remaining_variable_storage_size: *mut u64,
    maximum_variable_size: *mut u64,
) -> Status {
    log_call!(
        QueryVariableInfo,
        attributes,
        maximum_variable_storage_size,
        remaining_variable_storage_size,
        maximum_variable_size
    );
    Status::UNSUPPORTED
}

/// SimpleTextOutputProtocol functions.
unsafe extern "efiapi" fn reset(this: *mut SimpleTextOutputProtocol, extended: bool) -> Status {
    log_call!(SimpleTextOutputProtocol_Reset, this, extended);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn output_string(
    this: *mut SimpleTextOutputProtocol,
    raw: *const Char16,
) -> Status {
    log_call!(SimpleTextOutputProtocol_OutputString, this, raw);
    if raw.is_null() {
        return Status::INVALID_PARAMETER;
    }

    let mut chars = Vec::new();
    for i in 0..80 {
        // SAFETY: This is safe as raw pointer 'raw' is not null.
        let c = unsafe { *raw.offset(i) };
        if c == 0 {
            break;
        }
        chars.push(c);
    }

    let s = core::char::decode_utf16(chars)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect::<String>();
    info!("{s}");

    Status::SUCCESS
}
unsafe extern "efiapi" fn test_string(
    this: *mut SimpleTextOutputProtocol,
    string: *const Char16,
) -> Status {
    log_call!(SimpleTextOutputProtocol_TestString, this, string);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn query_mode(
    this: *mut SimpleTextOutputProtocol,
    mode: usize,
    columns: *mut usize,
    rows: *mut usize,
) -> Status {
    log_call!(SimpleTextOutputProtocol_QueryMode, this, mode, columns, rows);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_mode(this: *mut SimpleTextOutputProtocol, mode: usize) -> Status {
    log_call!(SimpleTextOutputProtocol_SetMode, this, mode);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_attribute(
    this: *mut SimpleTextOutputProtocol,
    attribute: usize,
) -> Status {
    log_call!(SimpleTextOutputProtocol_SetAttribute, this, attribute);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn clear_screen(this: *mut SimpleTextOutputProtocol) -> Status {
    log_call!(SimpleTextOutputProtocol_CleanScreen, this);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_cursor_position(
    this: *mut SimpleTextOutputProtocol,
    column: usize,
    row: usize,
) -> Status {
    log_call!(SimpleTextOutputProtocol_SetCursorPosition, this, column, row);
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn enable_cursor(
    this: *mut SimpleTextOutputProtocol,
    visible: bool,
) -> Status {
    log_call!(SimpleTextOutputProtocol_EnableCursor, this, visible);
    Status::UNSUPPORTED
}

fn push_to_config_table(vendor_guid: Guid, vendor_table: *mut c_void) {
    let configuration_table_entry = ConfigurationTable { vendor_guid, vendor_table };
    let mut efi_loader = EFI_LOADER.lock();
    efi_loader.configuration_table.push(configuration_table_entry);
    efi_loader.system_table.configuration_table = efi_loader.configuration_table.as_mut_ptr();
    efi_loader.system_table.number_of_configuration_table_entries =
        efi_loader.configuration_table.len();
}

/// Sets the pointer to a protocol interface based on the provided protocol GUID.
///
/// Queries supported protocols and if it finds a match with provided protocol GUID, sets the
/// pointer to a particular protocol interface.
///
/// Returns UNSUPPORTED if no match is found.
fn set_protocol_interface_pointer(proto_guid: Guid, out_proto: *mut *mut c_void) -> Status {
    if out_proto.is_null() {
        return Status::UNSUPPORTED;
    };

    let loaded_image_protocol_ptr =
        &mut EFI_LOADER.lock().loaded_image_protocol as *mut _ as *mut c_void;

    // SAFETY: This is safe as raw pointer 'out_proto' is not null.
    let out_proto = unsafe { &mut *out_proto };

    *out_proto = match proto_guid {
        LOADED_IMAGE_PROTOCOL_GUID => loaded_image_protocol_ptr,
        LINUX_EFI_LOADED_IMAGE_FIXED_GUID => null_mut(),
        _ => return Status::UNSUPPORTED,
    };

    Status::SUCCESS
}

fn get_rt_properties_table_ptr() -> *mut c_void {
    let efi_loader = EFI_LOADER.lock();
    &efi_loader.rt_properties_table as *const _ as *mut c_void
}

fn add_allocated_pages(n: usize) -> Option<usize> {
    // SAFETY: TODO
    let allocated_pages = unsafe { ALLOCATED_PAGES } + n;
    let free_mem_page_count = FREE_COUNT as usize;
    if allocated_pages > free_mem_page_count {
        return None;
    }
    // SAFETY: TODO
    unsafe { ALLOCATED_PAGES = allocated_pages };
    Some(allocated_pages)
}

fn get_allocated_pages() -> usize {
    // SAFETY: TODO
    unsafe { ALLOCATED_PAGES }
}

fn increment_mem_map_counter() {
    MEM_MAP_KEY_COUNTER.fetch_add(1, Ordering::Relaxed);
}
