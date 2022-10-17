// Copyright 2022, The Android Open Source Project
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

//! Miscellaneous helper functions.

use vmbase::layout;

pub const SIZE_4KB: usize = 4 << 10;
pub const SIZE_2MB: usize = 2 << 20;

/// Computes the largest multiple of the provided alignment smaller or equal to the address.
pub const fn align_down(addr: usize, alignment: usize) -> usize {
    addr & !(alignment - 1)
}

/// Computes the first address larger or equal to the provided one that is aligned.
pub const fn align(addr: usize, alignment: usize) -> usize {
    align_down(addr + alignment, alignment)
}

/// Computes the address of the page containing a given address.
pub const fn page_of(addr: usize, page_size: usize) -> usize {
    align_down(addr, page_size)
}

/// Validates a page size and computes the address of the page containing a given address.
pub const fn checked_page_of(addr: usize, page_size: usize) -> Option<usize> {
    if page_size.is_power_of_two() {
        Some(page_of(addr, page_size))
    } else {
        None
    }
}

/// Computes the address of the 4KiB page containing a given address.
pub const fn page_4kb_of(addr: usize) -> usize {
    page_of(addr, SIZE_4KB)
}

/// Aligns the address to the next page boundary.
pub const fn page_align(addr: usize, page_size: usize) -> Option<usize> {
    if let Some(addr_in_next_page) = addr.checked_add(page_size) {
        checked_page_of(addr_in_next_page, page_size)
    } else {
        None
    }
}

/// Aligns the address to the next 4KiB page boundary.
pub const fn page_align_4kb(addr: usize) -> Option<usize> {
    page_align(addr, SIZE_4KB)
}

/// Gets a pointer to the first byte of the 4KiB-aligned payload appended to pvmfw's binary.
pub fn locate_appended_payload() -> usize {
    page_align_4kb(layout::binary_end())
        .expect("The page following the binary should never cause an address overflow")
}

/// Get size of the region that may contain the appended payload.
pub fn max_appended_payload_size() -> usize {
    let addr = locate_appended_payload();
    // pvmfw is contained in a 2MiB region so the payload can't be larger than the 2MiB alignement.
    align(addr, SIZE_2MB) - addr
}
