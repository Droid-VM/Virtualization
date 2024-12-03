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

//! Support for EFI boot services.

use core::ffi::c_void;
use core::mem;
use core::ptr::null_mut;
use uefi_raw::protocol::device_path::DevicePathProtocol;
use uefi_raw::table::boot::{
    BootServices, EventNotifyFn, EventType, InterfaceType, MemoryDescriptor, MemoryType,
    OpenProtocolInformationEntry, Tpl,
};
use uefi_raw::table::{Header, Revision};
use uefi_raw::{Char16, Event, Guid, Handle, PhysicalAddress, Status};

use crate::uefi::EFI_SPECIFICATION_REVISION;

const BOOT_SERVICES_SIGNATURE: u64 = 0x5652_4553_544f_4f42;

pub const fn init_boot_services() -> BootServices {
    BootServices {
        header: Header {
            signature: BOOT_SERVICES_SIGNATURE,
            revision: Revision(EFI_SPECIFICATION_REVISION),
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
    }
}

/// Task Priority functions.
unsafe extern "efiapi" fn raise_tpl(_new_tpl: Tpl) -> Tpl {
    Tpl(0)
}

unsafe extern "efiapi" fn restore_tpl(_new_tpl: Tpl) {}

/// Memory allocation functions.
unsafe extern "efiapi" fn allocate_pages(
    _alloc_ty: u32,
    _mem_ty: MemoryType,
    _count: usize,
    _addr: *mut PhysicalAddress,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn free_pages(_addr: PhysicalAddress, _pages: usize) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn get_memory_map(
    _size: *mut usize,
    _map: *mut MemoryDescriptor,
    _key: *mut usize,
    _desc_size: *mut usize,
    _desc_version: *mut u32,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn allocate_pool(
    _pool_type: MemoryType,
    _size: usize,
    _buffer: *mut *mut u8,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn free_pool(_buffer: *mut u8) -> Status {
    Status::UNSUPPORTED
}

/// Event & timer functions.
unsafe extern "efiapi" fn create_event(
    _ty: EventType,
    _notify_tpl: Tpl,
    _notify_func: Option<EventNotifyFn>,
    _notify_ctx: *mut c_void,
    _out_event: *mut Event,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn set_timer(_event: Event, _ty: u32, _trigger_time: u64) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn wait_for_event(
    _number_of_events: usize,
    _events: *mut Event,
    _out_index: *mut usize,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn signal_event(_event: Event) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn close_event(_event: Event) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn check_event(_event: Event) -> Status {
    Status::UNSUPPORTED
}

/// Protocol handler functions.
unsafe extern "efiapi" fn install_protocol_interface(
    _handle: *mut Handle,
    _guid: *const Guid,
    _interface_type: InterfaceType,
    _interface: *const c_void,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn reinstall_protocol_interface(
    _handle: Handle,
    _protocol: *const Guid,
    _old_interface: *const c_void,
    _new_interface: *const c_void,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn uninstall_protocol_interface(
    _handle: Handle,
    _protocol: *const Guid,
    _interface: *const c_void,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn handle_protocol(
    _handle: Handle,
    _proto: *const Guid,
    _out_proto: *mut *mut c_void,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn register_protocol_notify(
    _protocol: *const Guid,
    _event: Event,
    _registration: *mut *const c_void,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn locate_handle(
    _search_ty: i32,
    _proto: *const Guid,
    _key: *const c_void,
    _buf_sz: *mut usize,
    _buf: *mut Handle,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn locate_device_path(
    _proto: *const Guid,
    _device_path: *mut *const DevicePathProtocol,
    _out_handle: *mut Handle,
) -> Status {
    Status::NOT_FOUND
}

unsafe extern "efiapi" fn install_configuration_table(
    _guid_entry: *const Guid,
    _table_ptr: *const c_void,
) -> Status {
    Status::UNSUPPORTED
}

/// Image service functions.
unsafe extern "efiapi" fn load_image(
    _boot_policy: u8,
    _parent_image_handle: Handle,
    _device_path: *const DevicePathProtocol,
    _source_buffer: *const u8,
    _source_size: usize,
    _image_handle: *mut Handle,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn start_image(
    _image_handle: Handle,
    _exit_data_size: *mut usize,
    _exit_data: *mut *mut Char16,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn exit(
    _image_handle: Handle,
    _exit_status: Status,
    _exit_data_size: usize,
    _exit_data: *mut Char16,
) -> ! {
    panic!()
}

unsafe extern "efiapi" fn unload_image(_image_handle: Handle) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn exit_boot_services(_image_handle: Handle, _map_key: usize) -> Status {
    Status::UNSUPPORTED
}

/// Misc service functions.
unsafe extern "efiapi" fn get_next_monotonic_count(_count: *mut u64) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn stall(_microseconds: usize) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn set_watchdog_timer(
    _timeout: usize,
    _watchdog_code: u64,
    _data_size: usize,
    _watchdog_data: *const u16,
) -> Status {
    Status::UNSUPPORTED
}

// Driver support service functions.
unsafe extern "efiapi" fn connect_controller(
    _controller: Handle,
    _driver_image: Handle,
    _remaining_device_path: *const DevicePathProtocol,
    _recursive: bool,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn disconnect_controller(
    _controller: Handle,
    _driver_image: Handle,
    _child: Handle,
) -> Status {
    Status::UNSUPPORTED
}

/// Protocol open / close service functions.
unsafe extern "efiapi" fn open_protocol(
    _handle: Handle,
    _protocol: *const Guid,
    _interface: *mut *mut c_void,
    _agent_handle: Handle,
    _controller_handle: Handle,
    _attributes: u32,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn close_protocol(
    _handle: Handle,
    _protocol: *const Guid,
    _agent_handle: Handle,
    _controller_handle: Handle,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn open_protocol_information(
    _handle: Handle,
    _protocol: *const Guid,
    _entry_buffer: *mut *const OpenProtocolInformationEntry,
    _entry_count: *mut usize,
) -> Status {
    Status::UNSUPPORTED
}

/// Library service functions.
unsafe extern "efiapi" fn protocols_per_handle(
    _handle: Handle,
    _protocol_buffer: *mut *mut *const Guid,
    _protocol_buffer_count: *mut usize,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn locate_handle_buffer(
    _search_ty: i32,
    _proto: *const Guid,
    _key: *const c_void,
    _no_handles: *mut usize,
    _buf: *mut *mut Handle,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn locate_protocol(
    _proto: *const Guid,
    _registration: *mut c_void,
    _out_proto: *mut *mut c_void,
) -> Status {
    Status::UNSUPPORTED
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
    _handle: *mut Handle,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "C" fn uninstall_multiple_protocol_interfaces_non_variadic(
    _handle: Handle,
) -> Status {
    Status::UNSUPPORTED
}

/// CRC service functions.
unsafe extern "efiapi" fn calculate_crc32(
    _data: *const c_void,
    _data_size: usize,
    _crc32: *mut u32,
) -> Status {
    Status::UNSUPPORTED
}

/// Misc service functions.
unsafe extern "efiapi" fn copy_mem(_dest: *mut u8, _src: *const u8, _len: usize) {}

unsafe extern "efiapi" fn set_mem(_buffer: *mut u8, _len: usize, _value: u8) {}

/// New event functions (UEFI 2.0 or newer).
unsafe extern "efiapi" fn create_event_ex(
    _ty: EventType,
    _notify_tpl: Tpl,
    _notify_fn: Option<EventNotifyFn>,
    _notify_ctx: *mut c_void,
    _event_group: *mut Guid,
    _out_event: *mut Event,
) -> Status {
    Status::UNSUPPORTED
}
