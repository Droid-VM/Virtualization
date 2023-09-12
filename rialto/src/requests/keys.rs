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

//! Handles the key generation.

use crate::error::RequestProcessingError;
use bssl_ffi::BN_CTX_free;
use bssl_ffi::BN_CTX_new;
use bssl_ffi::EC_KEY_free;
use bssl_ffi::EC_KEY_generate_key;
use bssl_ffi::EC_KEY_get_conv_form;
use bssl_ffi::EC_KEY_key2buf;
use bssl_ffi::EC_KEY_new_by_curve_name;
use bssl_ffi::EC_KEY_priv2buf;
use bssl_ffi::NID_X9_62_prime256v1; // EC P-256 CURVE Nid
use bssl_ffi::OPENSSL_free;
use bssl_ffi::BN_CTX;
use bssl_ffi::EC_KEY;
use core::mem::MaybeUninit;
use core::result;
use core::slice;

type Result<T> = result::Result<T, RequestProcessingError>;

pub struct EcKey(*mut EC_KEY);

impl EcKey {
    pub fn new_p256() -> Result<Self> {
        // SAFETY: The returned pointer is checked below.
        let key = unsafe { EC_KEY_new_by_curve_name(NID_X9_62_prime256v1) };
        if key.is_null() {
            return Err(RequestProcessingError::NullBoringSSLObject("EC_KEY"));
        }
        // SAFETY: We assume that the non-NULL pointer points to a valid `EC_KEY`.
        // The randomness is provided by `getentropy()` in `vmbase`.
        let ret = unsafe { EC_KEY_generate_key(key) };
        if ret == 1 {
            Ok(Self(key))
        } else {
            Err(RequestProcessingError::KeyGenerationFailed)
        }
    }

    pub fn public_key(&self) -> Result<Key> {
        let mut key_ptr = MaybeUninit::zeroed();
        let ctx = BnCtx::new()?;
        // SAFETY: We assume that the non-NULL pointer obtained at initialization points
        // to a valid `EC_KEY`.
        let len = unsafe {
            let conversion_form = EC_KEY_get_conv_form(self.0 as *const EC_KEY);
            EC_KEY_key2buf(self.0 as *const EC_KEY, conversion_form, key_ptr.as_mut_ptr(), ctx.0)
        };
        if len > 0 {
            // SAFETY: `key_ptr` should have been initialized now.
            let key_ptr = unsafe { key_ptr.assume_init() };
            Ok(Key::new(key_ptr, len))
        } else {
            Err(RequestProcessingError::GettingPublicKeyFailed)
        }
    }

    pub fn private_key(&self) -> Result<Key> {
        let mut key_ptr = MaybeUninit::zeroed();
        // SAFETY: We assume that the non-NULL pointer obtained at initialization points
        // to a valid `EC_KEY`.
        let len = unsafe { EC_KEY_priv2buf(self.0 as *const EC_KEY, key_ptr.as_mut_ptr()) };
        if len > 0 {
            // SAFETY: `key_ptr` should have been initialized now.
            let key_ptr = unsafe { key_ptr.assume_init() };
            Ok(Key::new(key_ptr, len))
        } else {
            Err(RequestProcessingError::GettingPrivateKeyFailed)
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

pub struct Key {
    /// Pointer to the buffer containing the key.
    ptr: *mut u8,
    len: usize,
}

impl Key {
    fn new(ptr: *mut u8, len: usize) -> Self {
        assert!(!ptr.is_null());
        Self { ptr, len }
    }

    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: We assume that the non-null pointer should point to a valid array of given
        // length.
        unsafe { slice::from_raw_parts(self.ptr as *const u8, self.len) }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: This is safe because `self.ptr` is checked nonnull when the instance is created.
        // We can free this pointer when the key is no longer needed.
        unsafe { OPENSSL_free(self.ptr as *mut _) }
    }
}

struct BnCtx(*mut BN_CTX);

impl BnCtx {
    fn new() -> Result<Self> {
        // SAFETY: The returned pointer is checked below.
        let ctx = unsafe { BN_CTX_new() };
        if ctx.is_null() {
            return Err(RequestProcessingError::NullBoringSSLObject("BN_CTX"));
        }
        Ok(Self(ctx))
    }
}

impl Drop for BnCtx {
    fn drop(&mut self) {
        // SAFETY: We assume that the non-NULL pointer obtained at initialization points
        // to a valid `BN_CTX`.
        unsafe { BN_CTX_free(self.0) }
    }
}
