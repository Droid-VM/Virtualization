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

use std::error::Error;
use std::ffi::{c_void, CStr};
use std::fmt::{self, Display};
use std::ptr::{self, NonNull};

use vm_payload_bindgen::{
    AVmAttestationResult, AVmAttestationResult_free, AVmAttestationStatus,
    AVmAttestationStatus_toString, AVmPayload_requestAttestation,
};

/// TODO
pub struct AttestationResult {
    result: NonNull<AVmAttestationResult>,
}

/// TODO
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum AttestationError {
    /// TODO
    InvalidChallenge,
    /// TODO
    AttestationFailed,
    /// TODO
    AttestationUnsupported,
}

impl Error for AttestationError {}

impl Display for AttestationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let status = match self {
            Self::InvalidChallenge => AVmAttestationStatus::ATTESTATION_ERROR_INVALID_CHALLENGE,
            Self::AttestationFailed => AVmAttestationStatus::ATTESTATION_ERROR_ATTESTATION_FAILED,
            Self::AttestationUnsupported => AVmAttestationStatus::ATTESTATION_ERROR_UNSUPPORTED,
        };
        // SAFETY: AVmAttestationStatus_toString always returns a non-null pointer to a
        // nul-terminated C string with static lifetime (which is valid UTF-8).
        let c_str = unsafe { CStr::from_ptr(AVmAttestationStatus_toString(status)) };
        let str = c_str.to_str().expect("Invalid UTF-8 for AVmAttestationStatus");
        f.write_str(str)
    }
}

impl Drop for AttestationResult {
    fn drop(&mut self) {
        // SAFETY: This field is private, and only populated with a successful call to
        // `AVmPayload_requestAttestation`, and not freed elsewhere.
        unsafe { AVmAttestationResult_free(self.result.as_ptr()) };
    }
}

/// TODO
pub fn request_attestation(challenge: &[u8]) -> Result<AttestationResult, AttestationError> {
    let mut result: *mut AVmAttestationResult = ptr::null_mut();
    // SAFETY: We only read the challenge within its bounds and the function does not retain any
    // reference to it.
    let status = unsafe {
        AVmPayload_requestAttestation(
            challenge.as_ptr() as *const c_void,
            challenge.len(),
            &mut result,
        )
    };
    match status {
        AVmAttestationStatus::ATTESTATION_ERROR_INVALID_CHALLENGE => {
            Err(AttestationError::InvalidChallenge)
        }
        AVmAttestationStatus::ATTESTATION_ERROR_ATTESTATION_FAILED => {
            Err(AttestationError::AttestationFailed)
        }
        AVmAttestationStatus::ATTESTATION_ERROR_UNSUPPORTED => {
            Err(AttestationError::AttestationUnsupported)
        }
        AVmAttestationStatus::ATTESTATION_OK => {
            let result = NonNull::new(result)
                .expect("Attestation succeeded but the attestation result is null");
            Ok(AttestationResult { result })
        }
    }
}
