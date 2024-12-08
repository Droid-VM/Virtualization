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

use crate::uefi::linux::LINUX_EFI_LOADED_IMAGE_FIXED_GUID;
use crate::uefi::loaded_image::LOADED_IMAGE_PROTOCOL_GUID;
use crate::uefi::EfiLoader;
use crate::uefi::{EFI_LOADER, EFI_SPECIFICATION_REVISION};
use core::ffi::c_void;
use core::mem;
use core::ptr::{copy, null_mut, write_bytes, NonNull};
use uefi_raw::protocol::device_path::DevicePathProtocol;
use uefi_raw::table::boot::{
    BootServices, EventNotifyFn, EventType, InterfaceType, MemoryDescriptor, MemoryType,
    OpenProtocolInformationEntry, Tpl,
};
use uefi_raw::table::{Header, Revision};
use uefi_raw::{Char16, Event, Guid, Handle, PhysicalAddress, Status};

use super::{non_null_and_aligned_const, non_null_and_aligned_mut};

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
    pool_type: MemoryType,
    size: usize,
    buffer: *mut *mut u8,
) -> Status {
    if size == 0 {
        return Status::INVALID_PARAMETER;
    }

    let result = EFI_LOADER.lock().allocate_pool(pool_type, size, buffer);
    match result {
        Ok(allocated) => {
            // Safety: This is safe as the raw pointer 'buffer' is not null.
            unsafe {
                buffer.write(allocated.as_ptr().cast());
            }
            Status::SUCCESS
        }
        Err(status) => status,
    }
}

unsafe extern "efiapi" fn free_pool(buffer: *mut u8) -> Status {
    if !non_null_and_aligned_mut(buffer) {
        return Status::INVALID_PARAMETER;
    }

    let void_buffer: *mut c_void = buffer as *mut c_void;
    let Some(ptr) = NonNull::new(void_buffer) else { return Status::INVALID_PARAMETER };
    vmbase::heap::deallocate(ptr);

    Status::SUCCESS
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
    handle: Handle,
    proto: *const Guid,
    out_proto: *mut *mut c_void,
) -> Status {
    if handle.is_null() || proto.is_null() || out_proto.is_null() {
        return Status::INVALID_PARAMETER;
    }
    // SAFETY: This is safe as raw pointer 'proto' is not null.
    let proto_guid = unsafe { *proto };

    // Check whether the received image handle matches the image handle pvmfw passed via
    // 'jump_to_payload_with_efi_stub' function before entering the EFI stub.
    let ptr = EfiLoader::EFI_IMAGE_HANDLE as *mut c_void;
    if handle != ptr {
        return Status::UNSUPPORTED;
    }

    set_protocol_interface_pointer(proto_guid, out_proto)
}

unsafe extern "efiapi" fn register_protocol_notify(
    _protocol: *const Guid,
    _event: Event,
    _registration: *mut *const c_void,
) -> Status {
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
            buf.write(EfiLoader::EFI_IMAGE_HANDLE as *mut c_void);
        }
    }

    Status::SUCCESS
}

unsafe extern "efiapi" fn locate_device_path(
    _proto: *const Guid,
    _device_path: *mut *const DevicePathProtocol,
    _out_handle: *mut Handle,
) -> Status {
    Status::UNSUPPORTED
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
    panic!("EFI payload called exit()");
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
    proto: *const Guid,
    _registration: *mut c_void,
    out_proto: *mut *mut c_void,
) -> Status {
    if proto.is_null() || out_proto.is_null() {
        return Status::INVALID_PARAMETER;
    }

    // SAFETY: This is safe as raw pointer 'proto' is not null.
    let proto_guid = unsafe { *proto };

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
unsafe extern "efiapi" fn copy_mem(dest: *mut u8, src: *const u8, len: usize) {
    if !non_null_and_aligned_mut(dest) || !non_null_and_aligned_const(src) {
        panic!("CopyMem received NULL: src={src:?}, dest={dest:?}");
    }
    // SAFETY: 'src' and 'dest' are not null and are aligned.
    unsafe { copy(src, dest, len) };
}

unsafe extern "efiapi" fn set_mem(buffer: *mut u8, len: usize, value: u8) {
    if !non_null_and_aligned_mut(buffer) {
        panic!("set_mem received NULL: buffer={buffer:?}");
    }
    // SAFETY: 'buffer' is not null and is aligned.
    unsafe { write_bytes(buffer, value, len) };
}

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
