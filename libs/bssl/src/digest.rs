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

use bssl_error::{ApiName, Error, Result};
use bssl_ffi::{EVP_sha256, EVP_MD};

/// Message digester.
#[derive(Clone, Debug)]
pub struct Md(*const EVP_MD);

impl Md {
    /// Returns an `Md` wrapping a pointer obtained with `EVP_sha256()`.
    pub fn sha256() -> Result<Self> {
        // SAFETY: The function doesn't access any Rust variable and the
        // returned pointer was checked to be non-null.
        let ptr = unsafe { EVP_sha256() };
        if ptr.is_null() {
            Err(Error::CallFailed(ApiName::EVP_sha256))
        } else {
            Ok(Self(ptr))
        }
    }

    /// Returns the inner pointer to `EVP_MD`.
    pub fn as_ptr(&self) -> *const EVP_MD {
        self.0
    }
}
