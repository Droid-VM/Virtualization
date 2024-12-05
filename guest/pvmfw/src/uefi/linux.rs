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

use log::error;

/// ARM specific configuration of the Linux kernel image.
/// See https://docs.kernel.org/arch/arm64/booting.html#call-the-kernel-image and
/// https://github.com/torvalds/linux/blob/feffde684ac29a3b7aec82d2df850fbdbdee55e4/arch/arm64/kernel/efi-header.S#L29
/// for further information.
///
/// Function returns the address of the EFI payload entrypoint.
pub fn locate_linux_efi_entrypoint(payload_start: usize, payload_size: usize) -> Option<usize> {
    // Count where the actual EFI payload starts.
    const KERNEL_HEADER_SIZE: usize = 64;
    const PE_HEADER_SIZE: usize = 24;
    const PE_MAGIC: u32 = 0x4550;
    const PE_OPT_MAGIC_PE32PLUS: u16 = 0x020b;
    const PE32PLUS_FIELD_AOEP: usize = 16;
    const MIN_PAYLOAD_SIZE: usize = KERNEL_HEADER_SIZE + PE_HEADER_SIZE + PE32PLUS_FIELD_AOEP;

    if payload_size < MIN_PAYLOAD_SIZE {
        error!("Payload size is out of the bounds!");
        return None;
    }

    let pe_header = payload_start.wrapping_add(KERNEL_HEADER_SIZE);
    // SAFETY: 'pe_header' points to the valid location in memory.
    let pe_magic = unsafe { *(pe_header as *const u32) };
    if pe_magic != PE_MAGIC {
        error!("PE MAGIC is not correct: {pe_magic:#x}, expected: {PE_MAGIC:#x}");
        return None;
    }

    let pe_opt_header = pe_header.wrapping_add(PE_HEADER_SIZE);
    // SAFETY: 'pe_opt_header' points to the valid location in memory.
    let pe_opt_magic_pe32plus = unsafe { *(pe_opt_header as *const u16) };
    if pe_opt_magic_pe32plus != PE_OPT_MAGIC_PE32PLUS {
        error!("PE MAGIC PE32+ is not correct: {pe_opt_magic_pe32plus:#x}");
        return None;
    }

    let pe_ep_offset_field = pe_opt_header.wrapping_add(PE32PLUS_FIELD_AOEP);
    // SAFETY: 'pe_ep_offset_field' points to the valid location in memory.
    let pe_ep_offset = usize::try_from(unsafe { *(pe_ep_offset_field as *const u32) }).unwrap();

    if pe_ep_offset >= payload_size {
        error!("PE entrypoint offset is out of the bounds: {pe_ep_offset:#x}");
        return None;
    }

    Some(payload_start.wrapping_add(pe_ep_offset))
}
