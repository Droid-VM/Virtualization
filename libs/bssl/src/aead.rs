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

//! Wrappers of the AEAD functions in BoringSSL aead.h.

use crate::util::check_int_result;
use bssl_avf_error::{ApiName, Error, Result};
use bssl_ffi::{
    EVP_AEAD_CTX_free, EVP_AEAD_CTX_new, EVP_AEAD_CTX_open, EVP_AEAD_CTX_seal,
    EVP_AEAD_max_overhead, EVP_aead_aes_256_gcm_randnonce, EVP_AEAD, EVP_AEAD_CTX,
    EVP_AEAD_DEFAULT_TAG_LENGTH,
};
use core::ptr::NonNull;

/// Magic value indicating that the default tag length for an AEAD should be used to
/// initialize `AeadCtx`.
pub const AEAD_DEFAULT_TAG_LENGTH: usize = EVP_AEAD_DEFAULT_TAG_LENGTH as usize;

/// Represents an AEAD algorithm.
pub struct Aead(&'static EVP_AEAD);

impl Aead {
    /// This is AES-256 in Galois Counter Mode with internal nonce generation.
    /// The 12-byte nonce is appended to the tag and is generated internally.
    pub fn aes_256_gcm_randnonce() -> Self {
        // SAFETY: This function does not access any Rust variables and simply returns
        // a pointer to the static variable in BoringSSL.
        let p = unsafe { EVP_aead_aes_256_gcm_randnonce() };
        // SAFETY: The returned pointer should always be valid and points to a static
        // `EVP_AEAD`.
        Self(unsafe { &*p })
    }

    /// Returns the maximum number of additional bytes added by the act of sealing data.
    pub fn max_overhead(&self) -> usize {
        // SAFETY: This function only reads from self.
        unsafe { EVP_AEAD_max_overhead(self.0) }
    }
}

/// Represents an AEAD algorithm configuration.
pub struct AeadCtx {
    ctx: NonNull<EVP_AEAD_CTX>,
    aead: Aead,
}

impl Drop for AeadCtx {
    fn drop(&mut self) {
        // SAFETY: It is safe because the pointer has been created with `EVP_AEAD_CTX_new`
        // and isn't used after this.
        unsafe { EVP_AEAD_CTX_free(self.ctx.as_ptr()) }
    }
}

impl AeadCtx {
    /// Creates a new `AeadCtx` with the given `Aead` algorithm, `key` and `tag_len`.
    pub fn new(aead: Aead, key: &[u8], tag_len: usize) -> Result<Self> {
        // SAFETY: This function only reads the given data and the returned pointer is
        // checked below.
        let ctx = unsafe { EVP_AEAD_CTX_new(aead.0, key.as_ptr(), key.len(), tag_len) };
        let ctx = NonNull::new(ctx).ok_or(Error::CallFailed(ApiName::EVP_AEAD_CTX_new))?;
        Ok(Self { ctx, aead })
    }

    /// Encrypts and authenticates `data` and writes the result to `out`.
    /// The `out` length should be at least the `data` length plus the `max_overhead` of the
    /// `aead`, otherwise an error will be thrown.
    ///
    /// Returns the length of data been written to `out`.
    pub fn seal(&self, data: &[u8], nonce: &[u8], ad: &[u8], out: &mut [u8]) -> Result<usize> {
        let mut out_len = 0;
        // SAFETY: Only reads from/writes to the provided slices.
        // The null inputs are handled inside the function.
        let ret = unsafe {
            EVP_AEAD_CTX_seal(
                self.ctx.as_ptr(),
                out.as_mut_ptr(),
                &mut out_len,
                out.len(),
                nonce.as_ptr(),
                nonce.len(),
                data.as_ptr(),
                data.len(),
                ad.as_ptr(),
                ad.len(),
            )
        };
        check_int_result(ret, ApiName::EVP_AEAD_CTX_seal)?;
        if out_len <= out.len() {
            Ok(out_len)
        } else {
            Err(Error::CallFailed(ApiName::EVP_AEAD_CTX_seal))
        }
    }

    /// Authenticates `data` and decrypts it to `out`.
    /// The `out` length should be at least the `data` length, otherwise an error will be thrown.
    ///
    /// Returns the length of data been written to `out`.
    pub fn open(&self, data: &[u8], nonce: &[u8], ad: &[u8], out: &mut [u8]) -> Result<usize> {
        let mut out_len = 0;
        // SAFETY: Only reads from/writes to the provided slices.
        // `data` and `out` are checked to be non-alias internally.
        // The null inputs are handled internally.
        let ret = unsafe {
            EVP_AEAD_CTX_open(
                self.ctx.as_ptr(),
                out.as_mut_ptr(),
                &mut out_len,
                out.len(),
                nonce.as_ptr(),
                nonce.len(),
                data.as_ptr(),
                data.len(),
                ad.as_ptr(),
                ad.len(),
            )
        };
        check_int_result(ret, ApiName::EVP_AEAD_CTX_open)?;
        if out_len <= out.len() {
            Ok(out_len)
        } else {
            Err(Error::CallFailed(ApiName::EVP_AEAD_CTX_open))
        }
    }

    /// Returns the `Aead` represented by this `AeadCtx`.
    pub fn aead(&self) -> &Aead {
        &self.aead
    }
}
