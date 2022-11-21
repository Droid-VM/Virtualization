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

//! Missing libc symbols used by the library.

use core::ffi::{c_char, c_int, c_ulong, CStr};

fn rust_strtoul(s: *const c_char, s_end: *mut *const c_char, base: c_int) -> Option<c_ulong> {
    let base = base as c_ulong;
    if base == 0 {
        return None;
    }

    // SAFETY - libc requires that s be a valid C-string.
    let digits = unsafe { CStr::from_ptr(s) }
        .to_str()
        .ok()?
        .chars()
        .enumerate()
        .skip_while(|&(_, c)| c.is_whitespace())
        .take_while(|&(_, c)| c.is_digit(base as u32));

    let mut value: c_ulong = 0;
    let mut offset: usize = 0;
    for (i, c) in digits {
        value = value.checked_mul(base)?.checked_add(c.to_digit(base as u32)?.into())?;
        offset = i;
    }

    if !s_end.is_null() {
        unsafe { *s_end = s.add(offset) };
    }

    Some(value)
}

#[no_mangle]
pub extern "C" fn strtoul(s: *const c_char, s_end: *mut *const c_char, base: c_int) -> c_ulong {
    rust_strtoul(s, s_end, base).unwrap_or(0)
}
