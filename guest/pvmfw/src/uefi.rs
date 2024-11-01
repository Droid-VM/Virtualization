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
#![allow(clippy::redundant_field_names)]
#![allow(clippy::just_underscores_and_digits)]
#![allow(clippy::empty_loop)]
#![allow(improper_ctypes_definitions)]

use crate::uefi_deps::BootServices;
use crate::uefi_deps::Char16;
use crate::uefi_deps::DevicePathProtocol;
use crate::uefi_deps::Event;
use crate::uefi_deps::EventNotifyFn;
use crate::uefi_deps::EventType;
use crate::uefi_deps::Guid;
use crate::uefi_deps::Handle;
use crate::uefi_deps::Header;
use crate::uefi_deps::InterfaceType;
use crate::uefi_deps::MemoryDescriptor;
use crate::uefi_deps::MemoryType;
use crate::uefi_deps::OpenProtocolInformationEntry;
use crate::uefi_deps::PhysicalAddress;
use crate::uefi_deps::Revision;
use crate::uefi_deps::SimpleTextOutputProtocol;
use crate::uefi_deps::Status;
use crate::uefi_deps::SystemTable;
use crate::uefi_deps::Tpl;
use core::ffi::c_void;
use core::mem;
use core::ptr;
use core::ptr::addr_of_mut;
use log::info;

pub static mut SYSTEM_TABLE: SystemTable = SystemTable {
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

// SAFETY: TODO(nikolinailic).
unsafe impl Sync for SystemTable {}

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

// SAFETY: TODO(nikolinailic).
unsafe impl Sync for BootServices {}

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
    log_boot_service_call("AllocatePages");
    Status::SUCCESS
}
unsafe extern "efiapi" fn free_pages(addr: PhysicalAddress, pages: usize) -> Status {
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
    log_boot_service_call("GetMemoryMap");
    Status::SUCCESS
}
unsafe extern "efiapi" fn allocate_pool(
    pool_type: MemoryType,
    size: usize,
    buffer: *mut *mut u8,
) -> Status {
    log_boot_service_call("AllocatePool");
    Status::SUCCESS
}
unsafe extern "efiapi" fn free_pool(buffer: *mut u8) -> Status {
    log_boot_service_call("FreePool");
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
    log_boot_service_call("HandleProtocol");
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
    log_boot_service_call("CopyMem");
}
unsafe extern "efiapi" fn set_mem(buffer: *mut u8, len: usize, value: u8) {
    log_boot_service_call("SetMem");
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
    Status::SUCCESS
}
unsafe extern "efiapi" fn output_string(
    this: *mut SimpleTextOutputProtocol,
    string: *const Char16,
) -> Status {
    info!("{:?}", string);
    Status::SUCCESS
}
unsafe extern "efiapi" fn test_string(
    this: *mut SimpleTextOutputProtocol,
    string: *const Char16,
) -> Status {
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn query_mode(
    this: *mut SimpleTextOutputProtocol,
    mode: usize,
    columns: *mut usize,
    rows: *mut usize,
) -> Status {
    Status::SUCCESS
}
unsafe extern "efiapi" fn set_mode(this: *mut SimpleTextOutputProtocol, mode: usize) -> Status {
    Status::SUCCESS
}
unsafe extern "efiapi" fn set_attribute(
    this: *mut SimpleTextOutputProtocol,
    attribute: usize,
) -> Status {
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn clear_screen(this: *mut SimpleTextOutputProtocol) -> Status {
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn set_cursor_position(
    this: *mut SimpleTextOutputProtocol,
    column: usize,
    row: usize,
) -> Status {
    Status::UNSUPPORTED
}
unsafe extern "efiapi" fn enable_cursor(
    this: *mut SimpleTextOutputProtocol,
    visible: bool,
) -> Status {
    Status::UNSUPPORTED
}

// Make sure to log all UEFI boot service calls.
fn log_boot_service_call(service_name: &str) {
    info!("Called EFI Boot Service: {}", service_name);
}
