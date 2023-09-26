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

use crate::digest::{HashAlgorithm, MessageDigest};
use bssl_avf_error::{ApiName, Error, Result};
use bssl_ffi::HMAC;

const SHA256_LEN: usize = HashAlgorithm::Sha256.digest_len();

/// Computes the HMAC using SHA-256 for the given `data` with the given `key`.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<[u8; SHA256_LEN]> {
    hmac::<SHA256_LEN>(key, data, HashAlgorithm::Sha256)
}

/// Computes the HMAC for the given `data` with the given `key` and `algo`.
/// The output size `HASH_LEN` should correspond to the digest length of the given hash algorithm.
fn hmac<const HASH_LEN: usize>(
    key: &[u8],
    data: &[u8],
    algo: HashAlgorithm,
) -> Result<[u8; HASH_LEN]> {
    assert_eq!(algo.digest_len(), HASH_LEN);

    let mut out = [0u8; HASH_LEN];
    let mut out_len = 0;
    let digester: MessageDigest = algo.try_into()?;
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
