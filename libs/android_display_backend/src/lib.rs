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

//! Crate implementing crosvm GPU display for Android

/// doc
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct android_display_context {
    _bindgen_opaque_blob: [u32; 1usize],
}

/// doc
#[allow(non_camel_case_types)]
pub type android_display_error_callback_type =
    ::std::option::Option<unsafe extern "C" fn(message: *const ::std::os::raw::c_char)>;

/// doc
#[no_mangle]
pub extern "C" fn create_android_display_context(
    _service_name: *const ::std::os::raw::c_char,
    _service_name_len: ::std::os::raw::c_ulong,
    _error_callback: android_display_error_callback_type,
) -> *mut android_display_context {
    unimplemented!();
}

/// doc
#[no_mangle]
pub extern "C" fn destroy_android_display_context(
    _error_callback: android_display_error_callback_type,
    _self_: *mut *mut android_display_context,
) {
    unimplemented!();
}

/// doc
#[no_mangle]
pub extern "C" fn get_android_display_width(
    _error_callback: android_display_error_callback_type,
    _self_: *mut android_display_context,
) -> u32 {
    unimplemented!();
}

/// doc
#[no_mangle]
pub extern "C" fn get_android_display_height(
    _error_callback: android_display_error_callback_type,
    _self_: *mut android_display_context,
) -> u32 {
    unimplemented!();
}

/// doc
#[no_mangle]
pub extern "C" fn blit_android_display(
    _error_callback: android_display_error_callback_type,
    _self_: *mut android_display_context,
    _width: u32,
    _height: u32,
    _bytes: *mut u8,
    _size: usize,
) {
    unimplemented!();
}


