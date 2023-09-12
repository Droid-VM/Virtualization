// Copyright 2023, The Android Open Source Project
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

//! Contains struct and functions that wraps the API related to EC_KEY in
//! boringssl.

use crate::error::RequestProcessingError;
use bssl_ffi::CBB_cleanup;
use bssl_ffi::CBB_finish;
use bssl_ffi::CBB_init;
use bssl_ffi::EC_KEY_free;
use bssl_ffi::EC_KEY_generate_key;
use bssl_ffi::EC_KEY_marshal_private_key;
use bssl_ffi::EC_KEY_new_by_curve_name;
use bssl_ffi::NID_X9_62_prime256v1; // EC P-256 CURVE Nid
use bssl_ffi::OPENSSL_free;
use bssl_ffi::CBB;
use bssl_ffi::EC_KEY;
use core::mem::MaybeUninit;
use core::ptr;
use core::result;
use core::slice;

type Result<T> = result::Result<T, RequestProcessingError>;

pub struct EcKey(*mut EC_KEY);

impl EcKey {
    pub fn new_p256() -> Result<Self> {
        // SAFETY: The returned pointer is checked below.
        let key = unsafe { EC_KEY_new_by_curve_name(NID_X9_62_prime256v1) };
        if key.is_null() {
            return Err(RequestProcessingError::InternalError("EC_KEY_new"));
        }
        // SAFETY: We assume that the non-NULL pointer points to a valid `EC_KEY`.
        // The randomness is provided by `getentropy()` in `vmbase`.
        let ret = unsafe { EC_KEY_generate_key(key) };
        if ret == 1 {
            Ok(Self(key))
        } else {
            // SAFETY: `key` is non-null and we assume it points to a valid `EC_KEY`.
            unsafe {
                EC_KEY_free(key);
            }
            Err(RequestProcessingError::KeyGeneration)
        }
    }

    // TODO(b/300068317): Returns the CoseKey for the public key.

    /// Returns the DER-encoded private key (RFC 5915).
    pub fn private_key(&self) -> Result<Buffer> {
        const CBB_INITIAL_CAPACITY: usize = 128;
        let mut cbb = Cbb::new(CBB_INITIAL_CAPACITY)?;
        let enc_flags = 0;
        let ret =
            // SAFETY: The function only write bytes to the buffer managed by the valid `CBB`
            // object.
            // The second parameter is a non-NULL pointer and should point to a valid `EC_KEY`.
            unsafe { EC_KEY_marshal_private_key(cbb.as_mut(), self.0 as *const EC_KEY, enc_flags) };
        if ret == 1 {
            cbb.finish()
        } else {
            Err(RequestProcessingError::GettingPrivateKey)
        }
    }
}

impl Drop for EcKey {
    fn drop(&mut self) {
        // SAFETY: We assume that the non-NULL pointer obtained at initialization points
        // to a valid `EC_KEY`.
        unsafe { EC_KEY_free(self.0) }
    }
}

pub struct Buffer {
    /// Pointer to the buffer.
    ptr: *mut u8,
    /// Length of the buffer.
    len: usize,
}

impl Buffer {
    fn new(ptr: *mut u8, len: usize) -> Self {
        assert!(!ptr.is_null());
        assert!(len > 0);
        Self { ptr, len }
    }

    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: We assume that the non-null pointer should point to a valid array of given
        // length.
        unsafe { slice::from_raw_parts(self.ptr as *const u8, self.len) }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        // SAFETY: This is safe because `self.ptr` is checked nonnull when the instance is created.
        // We can free this pointer when the key is no longer needed.
        unsafe { OPENSSL_free(self.ptr as *mut _) }
    }
}

struct Cbb(CBB);

impl Cbb {
    fn new(initial_capacity: usize) -> Result<Self> {
        let mut cbb = MaybeUninit::uninit();
        // SAFETY: Initializes the CBB. The return is checked below.
        let ret = unsafe { CBB_init(cbb.as_mut_ptr(), initial_capacity) };
        if ret == 1 {
            // SAFETY: The CBB object should be initialized since `CBB_init` succeeds.
            Ok(Self(unsafe { cbb.assume_init() }))
        } else {
            Err(RequestProcessingError::InternalError("CBB_init"))
        }
    }

    fn finish(&mut self) -> Result<Buffer> {
        let mut out_data = ptr::null_mut();
        let mut out_len = 0;
        // SAFETY: This is safe because the CBB pointer is initialized with `CBB_init()` at the
        // creation.
        let ret = unsafe { CBB_finish(self.as_mut(), &mut out_data, &mut out_len) };
        if ret == 1 && !out_data.is_null() && out_len > 0 {
            Ok(Buffer::new(out_data, out_len))
        } else {
            Err(RequestProcessingError::InternalError("CBB_finish"))
        }
    }

    fn as_mut(&mut self) -> &mut CBB {
        &mut self.0
    }
}

impl Drop for Cbb {
    fn drop(&mut self) {
        // SAFETY: This is safe because the CBB pointer is initialized with `CBB_init()` at the
        // creation.
        unsafe { CBB_cleanup(&mut self.0) }
    }
}
