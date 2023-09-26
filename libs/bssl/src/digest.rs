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

//! Wrappers of the digest functions in BoringSSL digest.h.

use bssl_avf_error::{Error, Result};
use bssl_ffi::{EVP_sha256, EVP_sha512, EVP_MD, SHA256_DIGEST_LENGTH, SHA512_DIGEST_LENGTH};
use log::error;

/// Message digesters.
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Digester {
    Sha256,
    #[allow(dead_code)]
    Sha512,
}

impl Digester {
    /// Returns the length of the hash function output in octets.
    pub const fn hash_len(&self) -> usize {
        let len = match self {
            Self::Sha256 => SHA256_DIGEST_LENGTH,
            Self::Sha512 => SHA512_DIGEST_LENGTH,
        };
        len as usize
    }

    /// Returns the pointer to the corresponding `EVD_MD` in BoringSSL.
    ///
    /// Since `EVD_MD`s are static, we don't need to free the memory after the usage.
    pub fn as_ptr(&self) -> Result<*const EVP_MD> {
        // SAFETY: The function doesn't access any Rust variable and just returns
        // the pointer to the static variable in BoringSSL.
        let ptr = unsafe {
            match self {
                Self::Sha256 => EVP_sha256(),
                Self::Sha512 => EVP_sha512(),
            }
        };
        if ptr.is_null() {
            error!("Obtained a null pointer to EVP_MD for the digester: {:?}", self);
            Err(Error::InternalError)
        } else {
            Ok(ptr)
        }
    }
}
