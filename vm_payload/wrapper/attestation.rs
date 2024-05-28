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
use std::iter::FusedIterator;
use std::ptr::{self, NonNull};

use vm_payload_bindgen::{
    AVmAttestationResult, AVmAttestationResult_free, AVmAttestationResult_getCertificateAt,
    AVmAttestationResult_getCertificateCount, AVmAttestationResult_getPrivateKey,
    AVmAttestationStatus, AVmAttestationStatus_toString, AVmPayload_requestAttestation,
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
        let ptr = self.result.as_ptr();

        // SAFETY: The `result` field is private, and only populated with a successful call to
        // `AVmPayload_requestAttestation`, and not freed elsewhere.
        unsafe { AVmAttestationResult_free(ptr) };
    }
}

// SAFETY: There is no mutable data behind the `AVmAttestationResult` pointer (the only function
// that takes a mutable pointer to it is `AVmAttestationResult_free`). The API functions that
// accept the pointer are all safe to call from any thread.
unsafe impl Send for AttestationResult {}

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

impl AttestationResult {
    fn as_const_ptr(&self) -> *const AVmAttestationResult {
        self.result.as_ptr().cast_const()
    }

    /// TODO
    pub fn private_key(&self) -> Vec<u8> {
        let ptr = self.as_const_ptr();

        let size =
            // SAFETY: We own the `AVmAttestationResult` pointer, so it is valid. The function
            // writes no data since we pass a zero size, and null is explicitly allowed for the
            // destination in that case.
            unsafe { AVmAttestationResult_getPrivateKey(ptr, ptr::null_mut(), 0) };

        let mut private_key = vec![0u8; size];
        // SAFETY: We own the `AVmAttestationResult` pointer, so it is valid. The function only
        // writes within the bounds of `private_key`, which we just allocated so cannot be aliased.
        let size = unsafe {
            AVmAttestationResult_getPrivateKey(
                ptr,
                private_key.as_mut_ptr() as *mut c_void,
                private_key.len(),
            )
        };
        assert_eq!(size, private_key.len());
        private_key
    }

    /// TODO
    pub fn certificate_chain(&self) -> CertIterator {
        // SAFETY: We own the `AVmAttestationResult` pointer, so it is valid.
        let count = unsafe { AVmAttestationResult_getCertificateCount(self.as_const_ptr()) };

        CertIterator { result: self, count, current: 0 }
    }

    fn certificate(&self, index: usize) -> Vec<u8> {
        let ptr = self.as_const_ptr();

        let size =
            // SAFETY: We own the `AVmAttestationResult` pointer, so it is valid. The function
            // writes no data since we pass a zero size, and null is explicitly allowed for the
            // destination in that case. The function will panic if `index` is out of range (which
            // is safe).
            unsafe { AVmAttestationResult_getCertificateAt(ptr, index, ptr::null_mut(), 0) };

        let mut cert = vec![0u8; size];
        // SAFETY: We own the `AVmAttestationResult` pointer, so it is valid. The function only
        // writes within the bounds of `cert`, which we just allocated so cannot be aliased.
        let size = unsafe {
            AVmAttestationResult_getCertificateAt(
                ptr,
                index,
                cert.as_mut_ptr() as *mut c_void,
                cert.len(),
            )
        };
        assert_eq!(size, cert.len());
        cert
    }
}

/// TODO
pub struct CertIterator<'a> {
    result: &'a AttestationResult,
    count: usize,
    current: usize,
}

impl<'a> Iterator for CertIterator<'a> {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current < self.count {
            let cert = self.result.certificate(self.current);
            self.current += 1;
            Some(cert)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = self.count - self.current;
        (size, Some(size))
    }
}

impl<'a> ExactSizeIterator for CertIterator<'a> {}
impl<'a> FusedIterator for CertIterator<'a> {}
