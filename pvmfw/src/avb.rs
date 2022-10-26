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

//! Image verification.

use core::slice;

pub fn get_reference_public_key() -> &'static [u8] {
    // SAFETY - This function is the only way to access those variables (set by xxd/clang).
    unsafe { slice::from_raw_parts(&avbpubkey as *const u8, avbpubkey_len) }
}

extern "C" {
    static avbpubkey: u8;
    static avbpubkey_len: usize;
}
