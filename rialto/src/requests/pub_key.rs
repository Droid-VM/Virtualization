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

//! Handles the construction of the MACed public key.

use crate::error::RequestProcessingError;
use alloc::vec;
use alloc::vec::Vec;
use bssl_ffi::EVP_sha256;
use bssl_ffi::HMAC;
use core::result;
use coset::{iana, CborSerializable, CoseKey, CoseMac0Builder, HeaderBuilder};

type Result<T> = result::Result<T, RequestProcessingError>;

/// Returns the MACed public key.
pub fn build_maced_public_key(public_key: CoseKey, hmac_key: &[u8]) -> Result<Vec<u8>> {
    const ALGO: iana::Algorithm = iana::Algorithm::HMAC_256_256;

    let protected = HeaderBuilder::new().algorithm(ALGO).build();
    let cose_mac = CoseMac0Builder::new()
        .protected(protected)
        .payload(public_key.to_vec()?)
        .try_create_tag(&[], |data| hmac_sha256(hmac_key, data))? // no external_aad.
        .build();
    Ok(cose_mac.to_vec()?)
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let mut out = vec![0u8; 32];
    let mut out_len = 0;
    // SAFETY: The function shouldn't access any Rust variable and the returned value is accepted
    // as a potentially NULL pointer.
    let digester = unsafe { EVP_sha256() };
    if digester.is_null() {
        return Err(RequestProcessingError::InternalError("EVP_sha256"));
    }
    // SAFETY: Only reads from/writes to the provided slices and supports digester was checked not
    // be NULL.
    let ret = unsafe {
        HMAC(
            digester,
            key.as_ptr() as *const _,
            key.len(),
            data.as_ptr(),
            data.len(),
            out.as_mut_ptr(),
            &mut out_len,
        )
    };
    let out_len = usize::try_from(out_len).unwrap();
    if !ret.is_null() && out_len == out.len() {
        Ok(out)
    } else {
        Err(RequestProcessingError::InternalError("HMAC"))
    }
}
