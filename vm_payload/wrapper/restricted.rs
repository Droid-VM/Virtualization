/*
 * Copyright 2024 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! The functions declaredin this module are restricted to VMs created with a config file;
//! they will fail, or panic, if called in other VMs. The ability to create such VMs
//! requires the android.permission.USE_CUSTOM_VIRTUAL_MACHINE permission, and is
//! therefore not available to privileged or third party apps.
//!
//! These functions can be used by tests, if the permission is granted via shell.

pub use crate::attestation::request_attestation_for_testing;

use std::ffi::c_void;
use std::ptr;
use vm_payload_bindgen::AVmPayload_getDiceAttestationChain;

/// Returns the DICE attestation chain for the VM.
pub fn get_dice_attestation_chain() -> Vec<u8> {
    // SAFETY: The function doesn't write to any memory.
    let size = unsafe { AVmPayload_getDiceAttestationChain(ptr::null_mut(), 0) };
    let mut buffer = vec![0u8; size];
    // SAFETY: The function only writes to `buffer` within its bounds.
    let actual_size =
        unsafe { AVmPayload_getDiceAttestationChain(buffer.as_mut_ptr() as *mut c_void, size) };
    assert_eq!(size, actual_size);
    buffer
}
