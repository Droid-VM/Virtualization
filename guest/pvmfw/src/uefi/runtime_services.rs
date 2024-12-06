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

//! Support for EFI runtime services.

use core::ffi::c_void;
use core::mem;
use uefi_raw::capsule::CapsuleHeader;
use uefi_raw::table::boot::MemoryDescriptor;
use uefi_raw::table::runtime::{ResetType, RuntimeServices, TimeCapabilities, VariableAttributes};
use uefi_raw::table::{Header, Revision};
use uefi_raw::time::Time;
use uefi_raw::{Char16, Guid, PhysicalAddress, Status};

use crate::uefi::EFI_SPECIFICATION_REVISION;

const RUNTIME_SERVICES_SIGNATURE: u64 = 0x0565_2453_544e_5552;

pub const fn init_runtime_services() -> RuntimeServices {
    RuntimeServices {
        header: Header {
            signature: RUNTIME_SERVICES_SIGNATURE,
            revision: Revision(EFI_SPECIFICATION_REVISION),
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
    }
}

unsafe extern "efiapi" fn get_time(
    _time: *mut Time,
    _capabilities: *mut TimeCapabilities,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn set_time(_time: *const Time) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn get_wakeup_time(
    _enabled: *mut u8,
    _pending: *mut u8,
    _time: *mut Time,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn set_wakeup_time(_enable: u8, _time: *const Time) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn set_virtual_address_map(
    _map_size: usize,
    _desc_size: usize,
    _desc_version: u32,
    _virtual_map: *mut MemoryDescriptor,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn convert_pointer(
    _debug_disposition: usize,
    _address: *mut *const c_void,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn get_variable(
    _variable_name: *const Char16,
    _vendor_guid: *const Guid,
    _attributes: *mut VariableAttributes,
    _data_size: *mut usize,
    _data: *mut u8,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn get_next_variable_name(
    _variable_name_size: *mut usize,
    _variable_name: *mut u16,
    _vendor_guid: *mut Guid,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn set_variable(
    _variable_name: *const Char16,
    _vendor_guid: *const Guid,
    _attributes: VariableAttributes,
    _data_size: usize,
    _data: *const u8,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn get_next_high_monotonic_count(_high_count: *mut u32) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn reset_system(
    _rt: ResetType,
    _status: Status,
    _data_size: usize,
    _data: *const u8,
) -> ! {
    panic!("EFI payload called reset_system()");
}

unsafe extern "efiapi" fn update_capsule(
    _capsule_header_array: *const *const CapsuleHeader,
    _capsule_count: usize,
    _scatter_gather_list: PhysicalAddress,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn query_capsule_capabilities(
    _capsule_header_array: *const *const CapsuleHeader,
    _capsule_count: usize,
    _maximum_capsule_size: *mut u64,
    _reset_type: *mut ResetType,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn query_variable_info(
    _attributes: VariableAttributes,
    _maximum_variable_storage_size: *mut u64,
    _remaining_variable_storage_size: *mut u64,
    _maximum_variable_size: *mut u64,
) -> Status {
    Status::UNSUPPORTED
}
