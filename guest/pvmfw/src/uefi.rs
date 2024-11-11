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
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::mem;
use core::ptr;
use core::ptr::addr_of_mut;
use core::ptr::null;
use core::ptr::null_mut;
use core::ptr::NonNull;
use log::error;
use log::info;
use uefi_raw::protocol::rng::{RngAlgorithmType, RngProtocol};
use uefi_raw::table::boot::MemoryAttribute;
use vmbase::memory::MemoryTracker;

use vmbase::memory::MEMORY;
use vmbase::memory::SIZE_4KB;

use uefi_raw::guid;
use uefi_raw::protocol::console::SimpleTextOutputProtocol;
use uefi_raw::protocol::device_path::DevicePathProtocol;
use uefi_raw::protocol::loaded_image::LoadedImageProtocol;
use uefi_raw::table::boot::{
    BootServices, EventNotifyFn, EventType, InterfaceType, MemoryDescriptor, MemoryType,
    OpenProtocolInformationEntry, Tpl,
};
use uefi_raw::table::system::SystemTable;
use uefi_raw::table::{Header, Revision};
use uefi_raw::{Char16, Event, Guid, Handle, PhysicalAddress, Status};

use crate::entry;
use crate::entry::EFI_IMAGE_HANDLE;

use crate::entry::MEM_MAP_FREE;
use crate::entry::MEM_MAP_INITRD;
use crate::entry::MEM_MAP_KERNEL;

const LINUX_EFI_LOADED_IMAGE_FIXED: Guid = guid!("f5a37b6d-3344-42a5-b6bb-978648c1890a");
const LOADED_IMAGE_PROTOCOL_GUID: Guid = guid!("5b1b31a1-9562-11d2-8e3f-00a0c969723b");
const RNG_PROTOCOL_GUID: Guid = guid!("3152bca5-eade-433d-862e-c01cdc291f44");

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
        size: 0,
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

    runtime_services: ptr::null_mut(),
    // SAFETY: TODO(nikolinailic).
    boot_services: unsafe { addr_of_mut!(BOOT_SERVICES) },
    number_of_configuration_table_entries: 0,
    configuration_table: ptr::null_mut(),
};

pub static mut BOOT_SERVICES: BootServices = BootServices {
    header: Header {
        signature: 0,
        // Revision of the spec this table conforms to.
        revision: Revision(0),
        // The size in bytes of the entire table.
        size: 0,
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

// Memory allocation functions - boot services.
unsafe extern "efiapi" fn allocate_pages(
    alloc_ty: u32,
    mem_ty: MemoryType,
    count: usize,
    addr: *mut PhysicalAddress,
) -> Status {
    log_call!(AllocatePages, alloc_ty, mem_ty, count, addr);
    Status::SUCCESS
}
unsafe extern "efiapi" fn free_pages(addr: PhysicalAddress, pages: usize) -> Status {
    info!("{:#x}", addr);
    info!("{:?}", pages);

    log_boot_service_call("FreePages");
    Status::SUCCESS
}

unsafe extern "efiapi" fn get_memory_map(
    size: *mut usize,
    map: *mut MemoryDescriptor,
    key: *mut usize,
    desc_size: *mut usize,
    desc_version: *mut u32,
) -> Status {
    if size.is_null() || key.is_null() || desc_size.is_null() || desc_version.is_null() {
        error!("get_memory_map({size:?}, {map:?}, {key:?}, {desc_size:?}, {desc_version:?})");
        return Status::INVALID_PARAMETER;
    }
    let (kernel_phys_start, kernel_page_count) = MEM_MAP_KERNEL.get().unwrap();
    let (initrd_phys_start, initrd_page_count) = MEM_MAP_INITRD.get().unwrap();
    let (free_mem_start, free_mem_page_count) = MEM_MAP_FREE.get().unwrap();
    // let free_mem_start = *initrd_phys_start + *initrd_page_count *
    // u64::try_from(SIZE_4KB).unwrap(); let free_mem_page_count = 40000;
    let memory_map = [
        MemoryDescriptor {
            ty: MemoryType::LOADER_DATA,
            phys_start: *kernel_phys_start,
            virt_start: *kernel_phys_start,
            page_count: *kernel_page_count,
            att: MemoryAttribute::WRITE_BACK,
        },
        MemoryDescriptor {
            ty: MemoryType::LOADER_DATA,
            phys_start: *initrd_phys_start,
            virt_start: *initrd_phys_start,
            page_count: *initrd_page_count,
            att: MemoryAttribute::WRITE_BACK,
        },
        MemoryDescriptor {
            ty: MemoryType::CONVENTIONAL,
            phys_start: *free_mem_start,
            virt_start: *free_mem_start,
            page_count: *free_mem_page_count,
            att: MemoryAttribute::MORE_RELIABLE,
        },
    ];

    let memory_map_size = mem::size_of_val(&memory_map);
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
        unsafe { ptr::copy(memory_map.as_ptr(), map, memory_map_size) };
    } else {
        status = Status::BUFFER_TOO_SMALL;
    }
    // SAFETY: TODO(nikolinailic)
    unsafe {
        // info!("{:?}", memory_map_size);
        // info!("{:?}", *size);
        *size = memory_map_size;
        *key = 123456789;
        *desc_size = 40;
        *desc_version = 1;
    }

    // info!("{:?}", size);
    // info!("{:?}", map);
    // info!("{:?}", key);
    // info!("{:?}", desc_size);
    // info!("{:?}", desc_version);

    log_boot_service_call("GetMemoryMap");
    status
}
unsafe extern "efiapi" fn allocate_pool(
    pool_type: MemoryType,
    size: usize,
    buffer: *mut *mut u8,
) -> Status {
    if size == 0 {
        return Status::INVALID_PARAMETER;
    }

    if pool_type != MemoryType::LOADER_DATA {
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

    // log_boot_service_call("AllocatePool");
    Status::SUCCESS
}
unsafe extern "efiapi" fn free_pool(buffer: *mut u8) -> Status {
    let void_buffer: *mut c_void = buffer as *mut c_void;
    let Some(ptr) = NonNull::new(void_buffer) else { return Status::INVALID_PARAMETER };
    vmbase::heap::deallocate(ptr);
    // log_boot_service_call("FreePool");
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
    image_base: 0x1111_1115usize as _,
    image_size: 0,
    image_code_type: MemoryType::LOADER_DATA,
    image_data_type: MemoryType::LOADER_DATA,
    unload: Some(unload),
};

pub static mut RNG_PROTOCOL: RngProtocol = RngProtocol { get_info, get_rng };

unsafe extern "efiapi" fn unload(image_handle: Handle) -> Status {
    log_function_call("LoadedImageProtocol: UNLOAD");
    Status::SUCCESS
}

unsafe extern "efiapi" fn get_info(
    this: *mut RngProtocol,
    algorithm_list_size: *mut usize,
    algorithm_list: *mut RngAlgorithmType,
) -> Status {
    log_function_call("RngProtocol: GETINFO");
    Status::SUCCESS
}

unsafe extern "efiapi" fn get_rng(
    this: *mut RngProtocol,
    algorithm: *const RngAlgorithmType,
    value_length: usize,
    value: *mut u8,
) -> Status {
    log_function_call("RngProtocol: GETRNG");

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
    if handle == ptr::null_mut() || proto == ptr::null() || out_proto == ptr::null_mut() {
        return Status::INVALID_PARAMETER;
    }
    // Check whether the image handle matches what pvmfw passed via jump_to_payload_with_efi_stub
    // function before entering the EFI stub.
    let ptr: *mut c_void = EFI_IMAGE_HANDLE as *mut c_void;
    if handle != ptr {
        return Status::UNSUPPORTED;
    }

    // SAFETY: TODO(nikolinailic).
    let proto_guid = unsafe { *proto };

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
        RNG_PROTOCOL_GUID => rng_protocol_ptr,
        LINUX_EFI_LOADED_IMAGE_FIXED => return Status::UNSUPPORTED,
        _ => return Status::UNSUPPORTED,
    };

    // info!("{:?}", handle);
    // info!("{:?}", proto);
    // info!("{:?}", out_proto);

    // log_boot_service_call("HandleProtocol");
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
    log_boot_service_call("LocateHandle");
    Status::SUCCESS
}
unsafe extern "efiapi" fn locate_device_path(
    proto: *const Guid,
    device_path: *mut *const DevicePathProtocol,
    out_handle: *mut Handle,
) -> Status {
    log_boot_service_call("LocateDevicePath");
    Status::SUCCESS
}
unsafe extern "efiapi" fn install_configuration_table(
    guid_entry: *const Guid,
    table_ptr: *const c_void,
) -> Status {
    log_boot_service_call("InstallConfigurationTable");
    Status::SUCCESS
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
    log_boot_service_call("ExitBootServices");
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
    // SAFETY: TODO(nikolinailic).
    let proto_guid = unsafe { *proto };

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
        RNG_PROTOCOL_GUID => rng_protocol_ptr,
        LINUX_EFI_LOADED_IMAGE_FIXED => return Status::UNSUPPORTED,
        _ => return Status::UNSUPPORTED,
    };
    log_boot_service_call("LocateProtocol");
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
    if src.is_null() || dest.is_null() {
        error!("copy_mem received NULL: src={src:?}, dest={dest:?}");
        return;
    }
    // SAFETY: TODO(nikolinailic).
    unsafe { ptr::copy(src, dest, len) };
}
unsafe extern "efiapi" fn set_mem(buffer: *mut u8, len: usize, value: u8) {
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
    // log_function_call("OutputString");

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

// Make sure to log all UEFI boot service calls.
fn log_boot_service_call(service_name: &str) {
    info!("Called EFI Boot Service: {}", service_name);
}

// Make sure to log all UEFI function calls.
fn log_function_call(function_name: &str) {
    info!("Called EFI function: {}", function_name);
}
