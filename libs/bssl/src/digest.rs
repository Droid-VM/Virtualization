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

use bssl_avf_error::{ApiName, Error, Result};
use bssl_ffi::{EVP_MD_size, EVP_sha256, EVP_sha512, EVP_MD};
use log::error;

/// Message digester wrapping a `EVP_MD` pointer.
///
/// Since `EVD_MD`s are static, we don't need to free the memory after the usage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digester(*const EVP_MD);

impl Digester {
    /// Returns a Digetser implementing `SHA-256` algorithm.
    pub fn sha256() -> Result<Self> {
        // SAFETY: The function doesn't access any Rust variable and just returns
        // the pointer to the static variable in BoringSSL.
        Self::from_ptr(unsafe { EVP_sha256() }, ApiName::EVP_sha256)
    }

    /// Returns a Digetser implementing `SHA-512` algorithm.
    #[allow(dead_code)]
    pub fn sha512() -> Result<Self> {
        // SAFETY: The function doesn't access any Rust variable and just returns
        // the pointer to the static variable in BoringSSL.
        Self::from_ptr(unsafe { EVP_sha512() }, ApiName::EVP_sha512)
    }

    fn from_ptr(p: *const EVP_MD, api_name: ApiName) -> Result<Self> {
        if p.is_null() {
            error!("Obtained a null pointer to EVP_MD from the BoringSSL API: {api_name:?}");
            Err(Error::InternalError)
        } else {
            Ok(Self(p))
        }
    }

    /// Returns the digest size in bytes.
    pub fn size(&self) -> usize {
        // SAFETY: The inner pointer is fetched from EVP_* hash functions in BoringSSL digest.h
        unsafe { EVP_MD_size(self.0) }
    }

    /// Returns the inner pointer to `EVD_MD`.
    pub fn as_ptr(&self) -> *const EVP_MD {
        self.0
    }
}
