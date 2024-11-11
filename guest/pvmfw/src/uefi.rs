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

//! EFI stub in pvmfw.

#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]
#![allow(clippy::cmp_null)]
#![allow(clippy::redundant_field_names)]
#![allow(clippy::just_underscores_and_digits)]
#![allow(clippy::empty_loop)]
#![allow(improper_ctypes_definitions)]

use aarch64_paging::paging::Descriptor;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem;
use core::ptr;
use core::ptr::addr_of_mut;
use core::ptr::null;
use core::ptr::null_mut;
use core::ptr::NonNull;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use log::error;
use log::info;
use uefi_raw::protocol::rng::{RngAlgorithmType, RngProtocol};
use uefi_raw::table;
use uefi_raw::table::boot::MemoryAttribute;
use vmbase::layout::crosvm::MEM_START;
use vmbase::memory::MemoryTracker;

use vmbase::memory::MEMORY;
use vmbase::memory::SIZE_4KB;

use uefi_raw::capsule::CapsuleHeader;
use uefi_raw::guid;
use uefi_raw::protocol::console::SimpleTextOutputProtocol;
use uefi_raw::protocol::device_path::DevicePathProtocol;
use uefi_raw::protocol::loaded_image::LoadedImageProtocol;
use uefi_raw::table::boot::{
    BootServices, EventNotifyFn, EventType, InterfaceType, MemoryDescriptor, MemoryType,
    OpenProtocolInformationEntry, Tpl,
};
use uefi_raw::table::configuration::ConfigurationTable;
use uefi_raw::table::runtime::{ResetType, RuntimeServices, TimeCapabilities, VariableAttributes};
use uefi_raw::table::system::SystemTable;
use uefi_raw::table::{Header, Revision};
use uefi_raw::time::Time;
use uefi_raw::{Char16, Event, Guid, Handle, PhysicalAddress, Status};

use crate::entry;
use crate::entry::EFI_IMAGE_HANDLE;

// use crate::entry::MEM_MAP_FREE;
// use crate::entry::MEM_MAP_INITRD;
// use crate::entry::MEM_MAP_KERNEL;

const LINUX_EFI_LOADED_IMAGE_FIXED: Guid = guid!("f5a37b6d-3344-42a5-b6bb-978648c1890a");
const LOADED_IMAGE_PROTOCOL_GUID: Guid = guid!("5b1b31a1-9562-11d2-8e3f-00a0c969723b");
const RNG_PROTOCOL_GUID: Guid = guid!("3152bca5-eade-433d-862e-c01cdc291f44");
// const LINUX_EFI_RANDOM_SEED_TABLE_GUID: Guid = guid!("1ce1e5bc-7ceb-42f2-81e5-8aadf180f57b");
const RT_PROPERTIES_TABLE_GUID: Guid = guid!("eb66918a-7eef-402a-842e-931d21c38ae9");
const DEVICE_TREE_GUID: Guid = guid!("b1b621d5-f19c-41a5-830b-d9152c69aae0");

static MEM_MAP_KEY_COUNTER: AtomicUsize = AtomicUsize::new(0);

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

pub static mut SYSTEM_TABLE: SystemTable = SystemTable {
    header: Header {
        signature: SystemTable::SIGNATURE,
        // Revision of the spec this table conforms to.
        revision: Revision(0),
        // The size in bytes of the entire table.
        size: mem::size_of::<SystemTable>() as _,
        // 32-bit CRC-32-Castagnoli of the entire table,
        // calculated with this field set to 0.
        crc: 0,
        // Reserved field that must be set to 0.
        reserved: 0,
    },

    firmware_vendor: ptr::null_mut(),
    firmware_revision: 0,

    stdin_handle: ptr::null_mut(),
    stdin: ptr::null_mut(),

    stdout_handle: ptr::null_mut(),
    // SAFETY: TODO(nikolinailic).
    stdout: unsafe { addr_of_mut!(SIMPLE_TEXT_OUTPUT_PROTOCOL) },

    stderr_handle: ptr::null_mut(),
    stderr: unsafe { addr_of_mut!(SIMPLE_TEXT_OUTPUT_PROTOCOL) },

    // SAFETY: TODO(nikolinailic).
    runtime_services: unsafe { addr_of_mut!(RUNTIME_SERVICES) },
    // SAFETY: TODO(nikolinailic).
    boot_services: unsafe { addr_of_mut!(BOOT_SERVICES) },
    // SAFETY: TODO(nikolinailic).
    number_of_configuration_table_entries: 0,
    // SAFETY: TODO(nikolinailic).
    configuration_table: ptr::null_mut(),
};

// pub static mut CONFIGURATION_TABLE: ConfigurationTable = ConfigurationTable {
//     vendor_guid: LINUX_EFI_RANDOM_SEED_TABLE_GUID,
//     vendor_table: ptr::null_mut(),
// };

pub static mut CONFIGURATION_TABLE: Vec<ConfigurationTable> = Vec::new();

fn push_to_config_table(vendor_guid: Guid, vendor_table: *mut c_void) {
    let configuration_table = ConfigurationTable { vendor_guid, vendor_table };
    // SAFETY: TODO(nikolinailic).
    unsafe {
        CONFIGURATION_TABLE.push(configuration_table);
        SYSTEM_TABLE.configuration_table = CONFIGURATION_TABLE.as_mut_ptr();
        SYSTEM_TABLE.number_of_configuration_table_entries = CONFIGURATION_TABLE.len();
    }
}

pub fn init_efi() {
    // info!("INITIALIZATION-----------------------------------------------------------------");
    push_to_config_table(
        RT_PROPERTIES_TABLE_GUID,
        // SAFETY: TODO.
        unsafe { addr_of_mut!(RT_PROPERTIES_TABLE) as _ },
    );
    push_to_config_table(DEVICE_TREE_GUID, 0x8fe00000 as *mut c_void);
}

pub struct RtPropertiesTable {
    pub version: u16,
    pub length: u16,
    pub runtime_services_supported: u32,
}

pub static mut RT_PROPERTIES_TABLE: RtPropertiesTable =
    RtPropertiesTable { version: 0, length: 8, runtime_services_supported: 0 };

pub static mut BOOT_SERVICES: BootServices = BootServices {
    header: Header {
        signature: 0,
        // Revision of the spec this table conforms to.
        revision: Revision(0),
        // The size in bytes of the entire table.
        size: mem::size_of::<BootServices>() as _,
        // 32-bit CRC-32-Castagnoli of the entire table,
        // calculated with this field set to 0.
        crc: 0,
        // Reserved field that must be set to 0.
        reserved: 0,
    },

    // Task Priority services.
    raise_tpl: raise_tpl,
    restore_tpl: restore_tpl,

    // Memory allocation functions.
    allocate_pages: allocate_pages,
    free_pages: free_pages,
    get_memory_map: get_memory_map,
    allocate_pool: allocate_pool,
    free_pool: free_pool,

    // Event & timer functions.
    create_event: create_event,
    set_timer: set_timer,
    wait_for_event: wait_for_event,
    signal_event: signal_event,
    close_event: close_event,
    check_event: check_event,

    // Protocol handlers.
    install_protocol_interface: install_protocol_interface,
    reinstall_protocol_interface: reinstall_protocol_interface,
    uninstall_protocol_interface: uninstall_protocol_interface,
    handle_protocol: handle_protocol,
    reserved: ptr::null_mut(),
    register_protocol_notify: register_protocol_notify,
    locate_handle: locate_handle,
    locate_device_path: locate_device_path,
    install_configuration_table: install_configuration_table,

    // Image services.
    load_image: load_image,
    start_image: start_image,
    exit: exit,
    unload_image: unload_image,
    exit_boot_services: exit_boot_services,

    // Misc services.
    get_next_monotonic_count: get_next_monotonic_count,
    stall: stall,
    set_watchdog_timer: set_watchdog_timer,

    // Driver support services.
    connect_controller: connect_controller,
    disconnect_controller: disconnect_controller,

    // Protocol open / close services.
    open_protocol: open_protocol,
    close_protocol: close_protocol,
    open_protocol_information: open_protocol_information,

    // Library services.
    protocols_per_handle: protocols_per_handle,
    locate_handle_buffer: locate_handle_buffer,
    locate_protocol: locate_protocol,

    // Warning: this function pointer is declared as `extern "C"` rather than
    // `extern "efiapi". That means it will work correctly when called from a
    // UEFI target (`*-unknown-uefi`), but will not work when called from a
    // target with a different calling convention such as
    // `x86_64-unknown-linux-gnu`.
    //
    // Support for C-variadics with `efiapi` requires the unstable
    // [`extended_varargs_abi_support`](https://github.com/rust-lang/rust/issues/100189)
    // feature.
    install_multiple_protocol_interfaces: unsafe {
        mem::transmute::<
            unsafe extern "C" fn(*mut Handle) -> Status,
            unsafe extern "C" fn(*mut Handle, ...) -> Status,
        >(install_multiple_protocol_interfaces)
    },

    // Warning: this function pointer is declared as `extern "C"` rather than
    // `extern "efiapi". That means it will work correctly when called from a
    // UEFI target (`*-unknown-uefi`), but will not work when called from a
    // target with a different calling convention such as
    // `x86_64-unknown-linux-gnu`.
    //
    // Support for C-variadics with `efiapi` requires the unstable
    // [`extended_varargs_abi_support`](https://github.com/rust-lang/rust/issues/100189)
    // feature.
    uninstall_multiple_protocol_interfaces: unsafe {
        mem::transmute::<
            unsafe extern "C" fn(Handle) -> Status,
            unsafe extern "C" fn(Handle, ...) -> Status,
        >(uninstall_multiple_protocol_interfaces)
    },

    // CRC services
    calculate_crc32: calculate_crc32,

    // Misc services
    copy_mem: copy_mem,
    set_mem: set_mem,

    // New event functions (UEFI 2.0 or newer)
    create_event_ex: create_event_ex,
};

pub static mut RUNTIME_SERVICES: RuntimeServices = RuntimeServices {
    header: Header {
        signature: 0,
        // Revision of the spec this table conforms to.
        revision: Revision(0),
        // The size in bytes of the entire table.
        size: mem::size_of::<RuntimeServices>() as _,
        // 32-bit CRC-32-Castagnoli of the entire table,
        // calculated with this field set to 0.
        crc: 0,
        // Reserved field that must be set to 0.
        reserved: 0,
    },
    get_time: get_time,
    set_time: set_time,
    get_wakeup_time: get_wakeup_time,
    set_wakeup_time: set_wakeup_time,
    set_virtual_address_map: set_virtual_address_map,
    convert_pointer: convert_pointer,
    get_variable: get_variable,
    get_next_variable_name: get_next_variable_name,
    set_variable: set_variable,
    get_next_high_monotonic_count: get_next_high_monotonic_count,
    reset_system: reset_system,
    update_capsule: update_capsule,
    query_capsule_capabilities: query_capsule_capabilities,
    query_variable_info: query_variable_info,
};

pub static mut SIMPLE_TEXT_OUTPUT_PROTOCOL: SimpleTextOutputProtocol = SimpleTextOutputProtocol {
    reset: reset,
    output_string: output_string,
    test_string: test_string,
    query_mode: query_mode,
    set_mode: set_mode,
    set_attribute: set_attribute,
    clear_screen: clear_screen,
    set_cursor_position: set_cursor_position,
    enable_cursor: enable_cursor,
    mode: ptr::null_mut(),
};

// Task Priority functions - boot services.
unsafe extern "efiapi" fn raise_tpl(new_tpl: Tpl) -> Tpl {
    log_boot_service_call("RaiseTpl");
    Tpl(0)
}
unsafe extern "efiapi" fn restore_tpl(new_tpl: Tpl) {
    log_boot_service_call("RestoreTpl");
}

const ALLOC_POINTER: usize = 0x8ce46000;

const KERNEL_START: u64 = 0x80000000;
const KERNEL_COUNT: u64 = 0x22a3;
const INITRD_START: u64 = 0x83000000;
const INITRD_COUNT: u64 = 0x206;
const FREE_START: u64 = 0x83206000;
const FREE_COUNT: u64 = 0x9c40;

// Memory allocation functions - boot services.
unsafe extern "efiapi" fn allocate_pages(
    alloc_ty: u32,
    mem_ty: MemoryType,
    count: usize,
    addr: *mut PhysicalAddress,
) -> Status {
    // SAFETY: TODO(nikolinailic).
    let addr_value = unsafe { *addr } as *const u8;
    log_call!(AllocatePages, alloc_ty, mem_ty, count, addr, addr_value);
    let offset = get_allocated_pages() * SIZE_4KB;
    let start = (ALLOC_POINTER + offset).try_into().unwrap();
    let _ = add_allocated_pages(count).unwrap();
    // SAFETY: TODO(nikolinailic).
    unsafe { addr.write(start) };

    info!("Allocated at {start:#x}");
    increment_mem_map_counter();

    Status::SUCCESS
}
unsafe extern "efiapi" fn free_pages(addr: PhysicalAddress, pages: usize) -> Status {
    log_call!(FreePages, addr, pages);
    Status::SUCCESS
}

static mut ALLOCATED_PAGES: usize = 0;

fn add_allocated_pages(n: usize) -> Option<usize> {
    // SAFETY: TODO
    let allocated_pages = unsafe { ALLOCATED_PAGES } + n;
    let free_mem_page_count = FREE_COUNT as usize;
    // let (_, free_mem_page_count) = MEM_MAP_FREE.get().unwrap();
    // let free_mem_page_count = usize::try_from(*free_mem_page_count).unwrap();
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

unsafe extern "efiapi" fn get_memory_map(
    size: *mut usize,
    map: *mut MemoryDescriptor,
    key: *mut usize,
    desc_size: *mut usize,
    desc_version: *mut u32,
) -> Status {
    log_call!(GetMemoryMap, size, map, key, desc_size, desc_version);
    if size.is_null() || key.is_null() || desc_size.is_null() || desc_version.is_null() {
        error!("Get_memory_map({size:?}, {map:?}, {key:?}, {desc_size:?}, {desc_version:?})");
        return Status::INVALID_PARAMETER;
    }
    let kernel_phys_start = KERNEL_START;
    let kernel_page_count = KERNEL_COUNT;
    let initrd_phys_start = INITRD_START;
    let initrd_page_count = INITRD_COUNT;
    let free_mem_start = FREE_START;
    let free_mem_page_count = FREE_COUNT;
    // let (kernel_phys_start, kernel_page_count) = MEM_MAP_KERNEL.get().unwrap();
    // let (initrd_phys_start, initrd_page_count) = MEM_MAP_INITRD.get().unwrap();
    // let (free_mem_start, free_mem_page_count) = MEM_MAP_FREE.get().unwrap();
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
        page_count: free_mem_page_count,
        att: MemoryAttribute::MORE_RELIABLE,
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
            page_count: free_mem_page_count - allocated_pages,
            att: MemoryAttribute::MORE_RELIABLE,
        })
    }
    let memory_map_size = memory_map.len() * mem::size_of::<MemoryDescriptor>();
    let memory_map_size_wrong = size_of_val(&memory_map);
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
        unsafe { ptr::copy(memory_map.as_ptr(), map, memory_map_size_wrong) };
    } else {
        status = Status::BUFFER_TOO_SMALL;
    }
    // SAFETY: TODO(nikolinailic)
    unsafe {
        info!("{:?}", memory_map_size);
        // info!("{:?}", *size);
        *size = memory_map_size;
        *key = MEM_MAP_KEY_COUNTER.load(Ordering::Relaxed);
        // info!("KEY: {:?}", MEM_MAP_KEY_COUNTER);
        // *key = 123456789;
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
    if size == 0 {
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

    // Safety:TODO
    unsafe {
        buffer.write(allocated.as_ptr().cast());
    }

    // let start = MEMORY.lock().as_mut().unwrap().find_region(size);

    // let range =
    //     MEMORY.lock().as_mut().unwrap().alloc_mut(start, core::num::NonZero::new(size).unwrap());

    // // Safety: TODO.
    // unsafe { *buffer = range.unwrap().start as *mut u8 };
    Status::SUCCESS
}
unsafe extern "efiapi" fn free_pool(buffer: *mut u8) -> Status {
    log_call!(FreePool, buffer);
    let void_buffer: *mut c_void = buffer as *mut c_void;
    let Some(ptr) = NonNull::new(void_buffer) else { return Status::INVALID_PARAMETER };
    vmbase::heap::deallocate(ptr);
    Status::SUCCESS
}

// Event & timer functions - boot services.
unsafe extern "efiapi" fn create_event(
    ty: EventType,
    notify_tpl: Tpl,
    notify_func: Option<EventNotifyFn>,
    notify_ctx: *mut c_void,
    out_event: *mut Event,
) -> Status {
    log_boot_service_call("CreateEvent");
    Status::SUCCESS
}
unsafe extern "efiapi" fn set_timer(event: Event, ty: u32, trigger_time: u64) -> Status {
    log_boot_service_call("SetTimer");
    Status::SUCCESS
}
unsafe extern "efiapi" fn wait_for_event(
    number_of_events: usize,
    events: *mut Event,
    out_index: *mut usize,
) -> Status {
    log_boot_service_call("WaitForEvent");
    Status::SUCCESS
}
unsafe extern "efiapi" fn signal_event(event: Event) -> Status {
    log_boot_service_call("SignalEvent");
    Status::SUCCESS
}
unsafe extern "efiapi" fn close_event(event: Event) -> Status {
    log_boot_service_call("CloseEvent");
    Status::SUCCESS
}
unsafe extern "efiapi" fn check_event(event: Event) -> Status {
    log_boot_service_call("CheckEvent");
    Status::SUCCESS
}

pub static mut LOADED_IMAGE_PROTOCOL: LoadedImageProtocol = LoadedImageProtocol {
    revision: 0,
    parent_handle: 0x1111_1111usize as _,
    system_table: unsafe { addr_of_mut!(SYSTEM_TABLE) },
    device_handle: 0x1111_1112usize as _,
    file_path: 0x1111_1113usize as _,
    reserved: ptr::null(),
    load_options_size: 0,
    load_options: 0x1111_1114usize as _,
    image_base: ptr::null(),
    image_size: 0,
    image_code_type: MemoryType::LOADER_DATA,
    image_data_type: MemoryType::LOADER_DATA,
    unload: Some(unload),
};

pub static mut RNG_PROTOCOL: RngProtocol = RngProtocol { get_info, get_rng };

unsafe extern "efiapi" fn unload(image_handle: Handle) -> Status {
    log_function_call("Called LoadedImageProtocol function: UNLOAD");
    Status::SUCCESS
}

unsafe extern "efiapi" fn get_info(
    this: *mut RngProtocol,
    algorithm_list_size: *mut usize,
    algorithm_list: *mut RngAlgorithmType,
) -> Status {
    log_function_call("Called RngProtocol function: GETINFO");
    Status::SUCCESS
}

unsafe extern "efiapi" fn get_rng(
    this: *mut RngProtocol,
    algorithm: *const RngAlgorithmType,
    value_length: usize,
    value: *mut u8,
) -> Status {
    log_call!(GetRng, this, algorithm, value_length, value);

    // SAFETY:TODO
    unsafe {
        *value = 15;
    }
    Status::SUCCESS
}

// Protocol handler functions - boot services.
unsafe extern "efiapi" fn install_protocol_interface(
    handle: *mut Handle,
    guid: *const Guid,
    interface_type: InterfaceType,
    interface: *const c_void,
) -> Status {
    log_boot_service_call("InstallProtocolInterface");
    Status::SUCCESS
}
unsafe extern "efiapi" fn reinstall_protocol_interface(
    handle: Handle,
    protocol: *const Guid,
    old_interface: *const c_void,
    new_interface: *const c_void,
) -> Status {
    log_boot_service_call("ReinstallProtocolInterface");
    Status::SUCCESS
}
unsafe extern "efiapi" fn uninstall_protocol_interface(
    handle: Handle,
    protocol: *const Guid,
    interface: *const c_void,
) -> Status {
    log_boot_service_call("UninstallProtocolInterface");
    Status::SUCCESS
}
unsafe extern "efiapi" fn handle_protocol(
    handle: Handle,
    proto: *const Guid,
    out_proto: *mut *mut c_void,
) -> Status {
    // SAFETY: TODO(nikolinailic).
    let proto_guid = unsafe { *proto };
    log_call!(HandleProtocol, handle, proto, proto_guid, out_proto);

    if handle == ptr::null_mut() || proto == ptr::null() || out_proto == ptr::null_mut() {
        return Status::INVALID_PARAMETER;
    }
    // Check whether the image handle matches what pvmfw passed via jump_to_payload_with_efi_stub
    // function before entering the EFI stub.
    let ptr: *mut c_void = EFI_IMAGE_HANDLE as *mut c_void;
    if handle != ptr {
        return Status::UNSUPPORTED;
    }

    let loaded_image_protocol_ptr: *mut c_void =
        // SAFETY: TODO(nikolinailic).
        unsafe { addr_of_mut!(LOADED_IMAGE_PROTOCOL) } as *mut c_void;
    let rng_protocol_ptr: *mut c_void =
        // SAFETY: TODO(nikolinailic).
        unsafe { addr_of_mut!(RNG_PROTOCOL) } as *mut c_void;

    // SAFETY: TODO
    let out_proto = unsafe { &mut *out_proto };

    *out_proto = match proto_guid {
        LOADED_IMAGE_PROTOCOL_GUID => loaded_image_protocol_ptr,
        // RNG_PROTOCOL_GUID => rng_protocol_ptr,
        LINUX_EFI_LOADED_IMAGE_FIXED => ptr::null_mut(),
        _ => return Status::UNSUPPORTED,
    };

    Status::SUCCESS
}
unsafe extern "efiapi" fn register_protocol_notify(
    protocol: *const Guid,
    event: Event,
    registration: *mut *const c_void,
) -> Status {
    log_boot_service_call("RegisterProtocolNotify");
    Status::SUCCESS
}
unsafe extern "efiapi" fn locate_handle(
    search_ty: i32,
    proto: *const Guid,
    key: *const c_void,
    buf_sz: *mut usize,
    buf: *mut Handle,
) -> Status {
    // SAFETY: TODO(nikolinailic).
    let proto_guid = unsafe { *proto };
    log_call!(LocateHandle, search_ty, proto, proto_guid, key, buf_sz, buf);
    if search_ty != 0 && search_ty != 1 && search_ty != 2 {
        return Status::INVALID_PARAMETER;
    }
    if (search_ty == 1 && key.is_null())
        || (search_ty == 2 && proto.is_null())
        || buf_sz.is_null()
        || proto.is_null()
    {
        return Status::INVALID_PARAMETER;
    }

    if search_ty == 2
        && proto_guid != RNG_PROTOCOL_GUID
        && proto_guid != LOADED_IMAGE_PROTOCOL_GUID
        && proto_guid != LINUX_EFI_LOADED_IMAGE_FIXED
    {
        return Status::NOT_FOUND;
    }

    // SAFETY: TODO(nikolinailic).
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

    // // SAFETY: TODO(nikolinailic).
    // unsafe {
    //     info!("Configuration Table Contents - BEFORE ADDING/REMOVING/UPDATING:");

    //     for (index, entry) in CONFIGURATION_TABLE.iter().enumerate() {
    //         info!("GUID: {:?}", entry.vendor_guid);
    //         info!("TABLE: {:?}", entry.vendor_table);
    //     }
    // }

    // SAFETY: TODO(nikolinailic).
    let proto_guid = unsafe { *guid_entry };

    info!("GUID: {:?}", proto_guid);

    // SAFETY: TODO(nikolinailic).
    unsafe {
        for (index, entry) in CONFIGURATION_TABLE.iter().enumerate() {
            if entry.vendor_guid == proto_guid && table_ptr.is_null() {
                // TODO: Remove this entry.
                return Status::SUCCESS;
            } else if entry.vendor_guid == proto_guid && !table_ptr.is_null() {
                // TODO: Update this entry.
                return Status::SUCCESS;
            }
        }
    }

    if !table_ptr.is_null() {
        push_to_config_table(proto_guid, table_ptr as _);
        // // SAFETY: TODO(nikolinailic).
        // unsafe {
        //     info!("Configuration Table Contents - AFTER ADDING/REMOVING/UPDATING:");

        //     for (index, entry) in CONFIGURATION_TABLE.iter().enumerate() {
        //         info!("GUID: {:?}", entry.vendor_guid);
        //         info!("TABLE: {:?}", entry.vendor_table);
        //     }
        // }
        Status::SUCCESS
    } else {
        Status::NOT_FOUND
    }
}

// Image service functions - boot services.
unsafe extern "efiapi" fn load_image(
    boot_policy: u8,
    parent_image_handle: Handle,
    device_path: *const DevicePathProtocol,
    source_buffer: *const u8,
    source_size: usize,
    image_handle: *mut Handle,
) -> Status {
    log_boot_service_call("LoadImage");
    Status::SUCCESS
}
unsafe extern "efiapi" fn start_image(
    image_handle: Handle,
    exit_data_size: *mut usize,
    exit_data: *mut *mut Char16,
) -> Status {
    log_boot_service_call("StartImage");
    Status::SUCCESS
}
unsafe extern "efiapi" fn exit(
    image_handle: Handle,
    exit_status: Status,
    exit_data_size: usize,
    exit_data: *mut Char16,
) -> ! {
    log_boot_service_call("Exit");
    loop {}
}
unsafe extern "efiapi" fn unload_image(image_handle: Handle) -> Status {
    log_boot_service_call("UnloadImage");
    Status::SUCCESS
}
unsafe extern "efiapi" fn exit_boot_services(image_handle: Handle, map_key: usize) -> Status {
    log_call!(ExitBootServices, image_handle, map_key);
    if map_key != MEM_MAP_KEY_COUNTER.load(Ordering::Relaxed) {
        return Status::INVALID_PARAMETER;
    }
    Status::SUCCESS
}

// Misc service functions - boot services.
unsafe extern "efiapi" fn get_next_monotonic_count(count: *mut u64) -> Status {
    log_boot_service_call("GetNextMonotonicCount");
    Status::SUCCESS
}
unsafe extern "efiapi" fn stall(microseconds: usize) -> Status {
    log_boot_service_call("Stall");
    Status::SUCCESS
}
unsafe extern "efiapi" fn set_watchdog_timer(
    timeout: usize,
    watchdog_code: u64,
    data_size: usize,
    watchdog_data: *const u16,
) -> Status {
    log_boot_service_call("SetWatchdogTimer");
    Status::SUCCESS
}

// Driver support service functions.
unsafe extern "efiapi" fn connect_controller(
    controller: Handle,
    driver_image: Handle,
    remaining_device_path: *const DevicePathProtocol,
    recursive: bool,
) -> Status {
    log_boot_service_call("ConnectController");
    Status::SUCCESS
}
unsafe extern "efiapi" fn disconnect_controller(
    controller: Handle,
    driver_image: Handle,
    child: Handle,
) -> Status {
    log_boot_service_call("DisconnectController");
    Status::SUCCESS
}

// Protocol open / close service functions - boot services.
unsafe extern "efiapi" fn open_protocol(
    handle: Handle,
    protocol: *const Guid,
    interface: *mut *mut c_void,
    agent_handle: Handle,
    controller_handle: Handle,
    attributes: u32,
) -> Status {
    log_boot_service_call("OpenProtocol");
    Status::SUCCESS
}
unsafe extern "efiapi" fn close_protocol(
    handle: Handle,
    protocol: *const Guid,
    agent_handle: Handle,
    controller_handle: Handle,
) -> Status {
    log_boot_service_call("CloseProtocol");
    Status::SUCCESS
}
unsafe extern "efiapi" fn open_protocol_information(
    handle: Handle,
    protocol: *const Guid,
    entry_buffer: *mut *const OpenProtocolInformationEntry,
    entry_count: *mut usize,
) -> Status {
    log_boot_service_call("OpenProtocolInformation");
    Status::SUCCESS
}

// Library service functions - boot services.
unsafe extern "efiapi" fn protocols_per_handle(
    handle: Handle,
    protocol_buffer: *mut *mut *const Guid,
    protocol_buffer_count: *mut usize,
) -> Status {
    log_boot_service_call("ProtocolsPerHandle");
    Status::SUCCESS
}
unsafe extern "efiapi" fn locate_handle_buffer(
    search_ty: i32,
    proto: *const Guid,
    key: *const c_void,
    no_handles: *mut usize,
    buf: *mut *mut Handle,
) -> Status {
    log_boot_service_call("LocateHandleBuffer");
    Status::SUCCESS
}
unsafe extern "efiapi" fn locate_protocol(
    proto: *const Guid,
    registration: *mut c_void,
    out_proto: *mut *mut c_void,
) -> Status {
    if proto.is_null() || out_proto.is_null() {
        return Status::INVALID_PARAMETER;
    }
    // SAFETY: TODO(nikolinailic).
    let proto_guid = unsafe { *proto };
    // let guid_str = proto_guid.to_ascii_hex_lower();

    log_call!(LocateProtocol, proto, proto_guid, registration, out_proto);

    let loaded_image_protocol_ptr: *mut c_void =
        // SAFETY: TODO(nikolinailic).
        unsafe { addr_of_mut!(LOADED_IMAGE_PROTOCOL) } as *mut c_void;
    let rng_protocol_ptr: *mut c_void =
        // SAFETY: TODO(nikolinailic).
        unsafe { addr_of_mut!(RNG_PROTOCOL) } as *mut c_void;

    // SAFETY: TODO
    let out_proto = unsafe { &mut *out_proto };

    *out_proto = match proto_guid {
        LOADED_IMAGE_PROTOCOL_GUID => loaded_image_protocol_ptr,
        // RNG_PROTOCOL_GUID => rng_protocol_ptr,
        LINUX_EFI_LOADED_IMAGE_FIXED => ptr::null_mut(),
        _ => {
            // info!("Guid: {:?}", proto_guid);
            return Status::NOT_FOUND;
        }
    };
    // info!("LOADED_IMAGE_PROTOCOL_GUID: {:?}", LOADED_IMAGE_PROTOCOL_GUID);
    // info!("LINUX_EFI_LOADED_IMAGE_FIXED: {:?}", LINUX_EFI_LOADED_IMAGE_FIXED);
    // info!("RNG_PROTOCOL_GUID: {:?}", RNG_PROTOCOL_GUID);
    // info!("PASSED: {:?}", proto_guid);

    Status::SUCCESS
}

// Warning: these functions are declared as `extern "C"` rather than
// `extern "efiapi". That means they will work correctly when called from a
// UEFI target (`*-unknown-uefi`), but will not work when called from a
// target with a different calling convention such as
// `x86_64-unknown-linux-gnu`.
//
// Support for C-variadics with `efiapi` requires the unstable
// [`extended_varargs_abi_support`](https://github.com/rust-lang/rust/issues/100189)
// feature.
unsafe extern "C" fn install_multiple_protocol_interfaces(handle: *mut Handle) -> Status {
    Status::SUCCESS
}
unsafe extern "C" fn uninstall_multiple_protocol_interfaces(handle: Handle) -> Status {
    Status::SUCCESS
}

// CRC service functions - boot services.
unsafe extern "efiapi" fn calculate_crc32(
    data: *const c_void,
    data_size: usize,
    crc32: *mut u32,
) -> Status {
    log_boot_service_call("CalculateCrc32");
    Status::SUCCESS
}

// Misc service functions - boot services.
unsafe extern "efiapi" fn copy_mem(dest: *mut u8, src: *const u8, len: usize) {
    log_call!(CopyMem, dest, src, len);
    if src.is_null() || dest.is_null() {
        error!("CopyMem received NULL: src={src:?}, dest={dest:?}");
        return;
    }
    // SAFETY: TODO(nikolinailic).
    unsafe { ptr::copy(src, dest, len) };
}
unsafe extern "efiapi" fn set_mem(buffer: *mut u8, len: usize, value: u8) {
    log_call!(SetMem, buffer, len, value);
    if buffer.is_null() {
        error!("set_mem received NULL: buffer={buffer:?}");
        return;
    }
    // SAFETY: TODO(nikolinailic)
    unsafe { ptr::write_bytes(buffer, value, len) };
}

// New event functions (UEFI 2.0 or newer) - boot services.
unsafe extern "efiapi" fn create_event_ex(
    ty: EventType,
    notify_tpl: Tpl,
    notify_fn: Option<EventNotifyFn>,
    notify_ctx: *mut c_void,
    event_group: *mut Guid,
    out_event: *mut Event,
) -> Status {
    log_boot_service_call("CreateEventEx");
    Status::SUCCESS
}

// SimpleTextOutputProtocol functions.
unsafe extern "efiapi" fn reset(this: *mut SimpleTextOutputProtocol, extended: bool) -> Status {
    log_function_call("Reset");
    Status::SUCCESS
}
unsafe extern "efiapi" fn output_string(
    this: *mut SimpleTextOutputProtocol,
    raw: *const Char16,
) -> Status {
    log_call!(OutputString, this, raw);

    let mut chars = Vec::new();
    // let raw = raw as *const u16;
    for i in 0..80 {
        // SAFETY: TODO()
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
    log_function_call("TestString");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn query_mode(
    this: *mut SimpleTextOutputProtocol,
    mode: usize,
    columns: *mut usize,
    rows: *mut usize,
) -> Status {
    log_function_call("QueryMode");
    Status::SUCCESS
}
unsafe extern "efiapi" fn set_mode(this: *mut SimpleTextOutputProtocol, mode: usize) -> Status {
    log_function_call("SetMode");
    Status::SUCCESS
}
unsafe extern "efiapi" fn set_attribute(
    this: *mut SimpleTextOutputProtocol,
    attribute: usize,
) -> Status {
    log_function_call("SetAttribute");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn clear_screen(this: *mut SimpleTextOutputProtocol) -> Status {
    log_function_call("CleanScreen");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_cursor_position(
    this: *mut SimpleTextOutputProtocol,
    column: usize,
    row: usize,
) -> Status {
    log_function_call("SetCursorPosition");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn enable_cursor(
    this: *mut SimpleTextOutputProtocol,
    visible: bool,
) -> Status {
    log_function_call("EnableCursor");
    Status::UNSUPPORTED
}

// Runtime services functions.
unsafe extern "efiapi" fn get_time(time: *mut Time, capabilities: *mut TimeCapabilities) -> Status {
    log_runtime_service_call("GetTime");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_time(time: *const Time) -> Status {
    log_runtime_service_call("SetTime");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn get_wakeup_time(
    enabled: *mut u8,
    pending: *mut u8,
    time: *mut Time,
) -> Status {
    log_runtime_service_call("GetWakeupTime");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_wakeup_time(enable: u8, time: *const Time) -> Status {
    log_runtime_service_call("SetWakeupTime");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_virtual_address_map(
    map_size: usize,
    desc_size: usize,
    desc_version: u32,
    virtual_map: *mut MemoryDescriptor,
) -> Status {
    log_runtime_service_call("SetVirtualAddressMap");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn convert_pointer(
    debug_disposition: usize,
    address: *mut *const c_void,
) -> Status {
    log_runtime_service_call("ConvertPointer");
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
    // SAFETY: TODO(nikolinailic) - PROBABLY NOT NEEDED.
    // unsafe {
    //     *data_size = 0;
    //     // info!("nv_seed_size: {:?}", *data_size);
    // }
    init_efi();
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn get_next_variable_name(
    variable_name_size: *mut usize,
    variable_name: *mut u16,
    vendor_guid: *mut Guid,
) -> Status {
    log_runtime_service_call("GetNextVariableName");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_variable(
    variable_name: *const Char16,
    vendor_guid: *const Guid,
    attributes: VariableAttributes,
    data_size: usize,
    data: *const u8,
) -> Status {
    log_runtime_service_call("SetVariable");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn get_next_high_monotonic_count(high_count: *mut u32) -> Status {
    log_runtime_service_call("GetNextHighMonotonicCount");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn reset_system(
    rt: ResetType,
    status: Status,
    data_size: usize,
    data: *const u8,
) -> ! {
    log_runtime_service_call("ResetSystem");
    loop {}
}
unsafe extern "efiapi" fn update_capsule(
    capsule_header_array: *const *const CapsuleHeader,
    capsule_count: usize,
    scatter_gather_list: PhysicalAddress,
) -> Status {
    log_runtime_service_call("UpdateCapsule");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn query_capsule_capabilities(
    capsule_header_array: *const *const CapsuleHeader,
    capsule_count: usize,
    maximum_capsule_size: *mut u64,
    reset_type: *mut ResetType,
) -> Status {
    log_runtime_service_call("QueryCapsuleCapabilites");
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn query_variable_info(
    attributes: VariableAttributes,
    maximum_variable_storage_size: *mut u64,
    remaining_variable_storage_size: *mut u64,
    maximum_variable_size: *mut u64,
) -> Status {
    log_runtime_service_call("QueryVariableInfo");
    Status::UNSUPPORTED
}

// Make sure to log all UEFI boot service calls.
fn log_boot_service_call(service_name: &str) {
    info!("Called EFI Boot Service: {}", service_name);
}

// Make sure to log all UEFI boot service calls.
fn log_runtime_service_call(service_name: &str) {
    info!("Called EFI Runtime Service: {}", service_name);
}

// Make sure to log all UEFI function calls.
fn log_function_call(function_name: &str) {
    info!("Called EFI function: {}", function_name);
}
