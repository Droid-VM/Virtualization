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

//! Wrappers of the HMAC functions in BoringSSL hmac.h.

use crate::digest::Md;
use bssl_error::{ApiName, Error, Result};
use bssl_ffi::{HMAC, SHA256_DIGEST_LENGTH};

const SHA256_HMAC_LEN: usize = SHA256_DIGEST_LENGTH as usize;

/// Computes the HMAC using SHA-256 for the given `data` with the given `key`.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<[u8; SHA256_HMAC_LEN]> {
    let digester = Md::sha256()?;
    hmac::<SHA256_HMAC_LEN>(key, data, &digester)
}

/// Computes the HMAC using SHA-256 for the given `data` with the given `key` and `digester`.
fn hmac<const N: usize>(key: &[u8], data: &[u8], digester: &Md) -> Result<[u8; N]> {
    let mut out = [0u8; N];
    let mut out_len = 0;
    // SAFETY: Only reads from/writes to the provided slices and the digester was checked
    // as non-null.
    let ret = unsafe {
        HMAC(
            digester.as_ptr(),
            key.as_ptr() as *const _,
            key.len(),
            data.as_ptr(),
            data.len(),
            out.as_mut_ptr(),
            &mut out_len,
        )
    };
    if !ret.is_null() && out_len == (out.len() as u32) {
        Ok(out)
    } else {
        Err(Error::CallFailed(ApiName::HMAC))
    }
}
