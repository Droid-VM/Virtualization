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

//! Support for EFI std I/O protocols.

use crate::uefi::non_null_and_aligned_const;
use alloc::string::String;
use core::char::decode_utf16;
use core::ptr::null_mut;
use core::slice;
use log::info;
use uefi_raw::protocol::console::SimpleTextOutputProtocol;
use uefi_raw::{Char16, Status};

pub const fn init_simple_text_output_protocol() -> SimpleTextOutputProtocol {
    SimpleTextOutputProtocol {
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
    }
}

unsafe extern "efiapi" fn reset(_this: *mut SimpleTextOutputProtocol, _extended: bool) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn output_string(
    _this: *mut SimpleTextOutputProtocol,
    raw: *const Char16,
) -> Status {
    if !non_null_and_aligned_const(raw) {
        return Status::INVALID_PARAMETER;
    }
    // SAFETY: We must trust that `raw` points to a valid null-terminated UTF16 string.
    let size = unsafe { utf16_len(raw) };
    // SAFETY: We trust that the slice covers a valid UTF16 string.
    let chars = unsafe { slice::from_raw_parts(raw, size) };

    let s = decode_utf16(chars.iter().cloned())
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect::<String>();

    for line in s.lines() {
        info!("[EFI] {}", line);
    }

    Status::SUCCESS
}

/// Returns the length (in `Char16`) of the null-terminated UTF16 string starting at `s`.
///
/// Result includes the first null character.
///
/// # Safety
///
/// `s` must point to a character within a valid null-terminated UTF16 string.
unsafe fn utf16_len(s: *const Char16) -> usize {
    for len in 0usize.. {
        let i = len.try_into().unwrap();
        // SAFETY: `s` points within a UTF16 string and we still haven't reached the end.
        let c = unsafe { s.offset(i) };
        // SAFETY: `c` points within the same UTF16 string as `s`.
        if unsafe { *c } == 0 {
            return len;
        }
    }
    0 // unreachable
}

unsafe extern "efiapi" fn test_string(
    _this: *mut SimpleTextOutputProtocol,
    _string: *const Char16,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn query_mode(
    _this: *mut SimpleTextOutputProtocol,
    _mode: usize,
    _columns: *mut usize,
    _rows: *mut usize,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn set_mode(_this: *mut SimpleTextOutputProtocol, _mode: usize) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn set_attribute(
    _this: *mut SimpleTextOutputProtocol,
    _attribute: usize,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn clear_screen(_this: *mut SimpleTextOutputProtocol) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn set_cursor_position(
    _this: *mut SimpleTextOutputProtocol,
    _column: usize,
    _row: usize,
) -> Status {
    Status::UNSUPPORTED
}

unsafe extern "efiapi" fn enable_cursor(
    _this: *mut SimpleTextOutputProtocol,
    _visible: bool,
) -> Status {
    Status::UNSUPPORTED
}
