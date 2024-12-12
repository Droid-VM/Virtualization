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

//! Support for Linux kernel image.

use super::EfiEntrypoint;
use core::mem;
use core::ptr;
use uefi_raw::{guid, Guid};
use zerocopy::{FromBytes, FromZeroes};

pub const LINUX_EFI_LOADED_IMAGE_FIXED_GUID: Guid = guid!("f5a37b6d-3344-42a5-b6bb-978648c1890a");
pub const RT_PROPERTIES_TABLE_GUID: Guid = guid!("eb66918a-7eef-402a-842e-931d21c38ae9");
pub const DEVICE_TREE_GUID: Guid = guid!("b1b621d5-f19c-41a5-830b-d9152c69aae0");

#[repr(C, packed)]
#[derive(FromBytes, FromZeroes)]
struct KernelHeader {
    code0_word0: u16,
    code0_word1: u16,
    code1: u32,
    image_load_offset: u64,
    kernel_size: u64,
    kernel_flags: u64,
    reserved0: u64,
    reserved1: u64,
    reserved2: u64,
    magic: u32,
    pe_header_offset: u32,
}

impl KernelHeader {
    const EFI_SIGNATURE: u16 = u16::from_le_bytes([b'M', b'Z']);
    const MAGIC: u32 = u32::from_le_bytes([b'A', b'R', b'M', 0x64]);
}

#[repr(C, packed)]
#[derive(FromBytes, FromZeroes)]
struct PeHeader {
    magic: u32,
    machine: u16,
    number_of_sections: u16,
    time_date_stamp: u32,
    pointer_to_symbol_table: u32,
    number_of_symbols: u32,
    size_of_optional_header: u16,
    characteristics: u16,
}

impl PeHeader {
    const MAGIC: u32 = 0x4550;
}

#[repr(C, packed)]
#[derive(FromBytes, FromZeroes)]
struct PeOptHeader {
    format: u16,
    major_linker_version: u8,
    minor_linker_version: u8,
    size_of_code: u32,
    size_of_initialized_data: u32,
    size_of_uninitialized_data: u32,
    address_of_entry_point: u32,
    base_of_code: u32,
    image_base: u64,
    section_alignment: u32,
    file_alignment: u32,
    major_operating_system_version: u16,
    minor_operating_system_version: u16,
    major_image_version: u16,
    minor_image_version: u16,
    major_subsystem_version: u16,
    minor_subsystem_version: u16,
    win32_version_value: u32,
    size_of_image: u32,
    size_of_headers: u32,
    check_sum: u32,
    subsystem: u16,
    dll_characteristics: u16,
    size_of_stack_reserve: u64,
    size_of_stack_commit: u64,
    size_of_heap_reserve: u64,
    size_of_heap_commit: u64,
    loader_flags: u32,
    number_of_rva_and_sizes: u32,
    export_table: u64,
    import_table: u64,
    resource_table: u64,
    exception_table: u64,
    certification_table: u64,
    base_relocation_table: u64,
}

impl PeOptHeader {
    const FORMAT: u16 = 0x020b;
}

/// ARM specific configuration of the Linux kernel image.
/// See arch/arm64/kernel/efi-header.S in the Linux kernel source for further information.
///
/// Function returns the address of the EFI payload entrypoint.
///
/// # Safety
///
/// Callers must ensure that the result of this function is properly executable before calling it.
pub unsafe fn locate_linux_efi_entrypoint(payload: &[u8]) -> Option<EfiEntrypoint> {
    let header = KernelHeader::ref_from_prefix(payload)?;
    if header.magic != KernelHeader::MAGIC || header.code0_word0 != KernelHeader::EFI_SIGNATURE {
        return None;
    }

    let pe_header_offset = header.pe_header_offset.try_into().unwrap();
    let pe_header = PeHeader::ref_from_prefix(&payload[pe_header_offset..])?;
    if pe_header.magic != PeHeader::MAGIC {
        return None;
    }

    let opt_header_offset = pe_header_offset + mem::size_of::<PeHeader>();
    let opt_header = PeOptHeader::ref_from_prefix(&payload[opt_header_offset..])?;
    if opt_header.format != PeOptHeader::FORMAT {
        return None;
    }

    let entrypoint_offset = usize::try_from(opt_header.address_of_entry_point).unwrap();
    let _ = payload.get(entrypoint_offset)?;

    let entrypoint = ptr::addr_of!(payload[entrypoint_offset]).cast::<u32>();
    if !entrypoint.is_aligned() {
        return None; // Invalid PC.
    }

    // SAFETY: The entrypoint slice should point to the valid location in the memory as the caller
    // had to perform sanity checks to ensure the validity of the location.
    Some(unsafe { mem::transmute::<*const u32, EfiEntrypoint>(entrypoint) })
}
