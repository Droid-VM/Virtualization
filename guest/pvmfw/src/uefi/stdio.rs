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

use core::ptr::null_mut;
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
    _raw: *const Char16,
) -> Status {
    Status::UNSUPPORTED
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
