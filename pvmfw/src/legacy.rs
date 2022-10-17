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

//! Support for legacy interfaces.

use crate::helpers;
use core::slice;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

/// Get a unique reference to the appended raw BCC.
pub fn take_bcc() -> Option<&'static mut [u8]> {
    static TAKEN: AtomicBool = AtomicBool::new(false);

    if !cfg!(feature = "legacy") || TAKEN.swap(true, Ordering::Relaxed) {
        None
    } else {
        let bcc = helpers::locate_appended_payload() as *mut u8;
        // SAFETY - This function is the only way to access the payload, which is prevented by the
        // linker script from aliasing or overlapping with other objects.
        Some(unsafe { slice::from_raw_parts_mut(bcc, helpers::SIZE_4KB) })
    }
}
