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
//! BoringSSL.

use alloc::vec::Vec;
use bssl_ffi::CBB_cleanup;
use bssl_ffi::CBB_finish;
use bssl_ffi::CBB_init;
use bssl_ffi::EC_KEY_check_key;
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
use service_vm_comm::{BoringSSLApiName, RequestProcessingError};
use zeroize::{Zeroize, ZeroizeOnDrop};

type Result<T> = result::Result<T, RequestProcessingError>;

/// Wrapper of an `EC_KEY` object, representing a public or private EC key.
pub struct EcKey(*mut EC_KEY);

fn check_result(ret: i32, api_name: BoringSSLApiName) -> Result<()> {
    if ret == 1 {
        Ok(())
    } else {
        Err(RequestProcessingError::BoringSSLCallFailed(api_name))
    }
}

impl EcKey {
    /// Creates a new EC P-256 key pair.
    pub fn new_p256() -> Result<Self> {
        // SAFETY: The returned pointer is checked below.
        let key = unsafe { EC_KEY_new_by_curve_name(NID_X9_62_prime256v1) };
        if key.is_null() {
            return Err(RequestProcessingError::BoringSSLCallFailed(
                BoringSSLApiName::EC_KEY_new_by_curve_name,
            ));
        }
        let mut ec_key = Self(key);
        ec_key.generate_key()?;
        ec_key.check_key()?;
        Ok(ec_key)
    }

    /// Generates a random, private key, calculates the corresponding public key and stores both
    /// in the `EC_KEY`.
    fn generate_key(&mut self) -> Result<()> {
        // SAFETY: The non-null pointer is created with `EC_KEY_new_by_curve_name` and should
        // point to a valid `EC_KEY`.
        // The randomness is provided by `getentropy()` in `vmbase`.
        check_result(unsafe { EC_KEY_generate_key(self.0) }, BoringSSLApiName::EC_KEY_generate_key)
    }

    /// Invokes `EC_KEY_check_key` that performs several checks on keys stored in `EC_KEY`.
    fn check_key(&mut self) -> Result<()> {
        // SAFETY: The non-null pointer is created with `EC_KEY_new_by_curve_name` and should
        // point to a valid `EC_KEY`.
        check_result(unsafe { EC_KEY_check_key(self.0) }, BoringSSLApiName::EC_KEY_check_key)
    }

    // TODO(b/300068317): Returns the CoseKey for the public key.

    /// Returns the DER-encoded ECPrivateKey structure described in RFC 5915 Section 3:
    ///
    /// https://datatracker.ietf.org/doc/html/rfc5915#section-3
    pub fn private_key(&self) -> Result<ZVec> {
        const CBB_INITIAL_CAPACITY: usize = 256;
        let mut cbb = Cbb::new(CBB_INITIAL_CAPACITY)?;
        let enc_flags = 0;
        let ret =
            // SAFETY: The function only write bytes to the buffer managed by the valid `CBB`
            // object, and the key has been allocated by BoringSSL.
            unsafe { EC_KEY_marshal_private_key(cbb.as_mut(), self.0, enc_flags) };
        check_result(ret, BoringSSLApiName::EC_KEY_marshal_private_key)?;
        cbb.finish()
    }
}

impl Drop for EcKey {
    fn drop(&mut self) {
        // SAFETY: It is safe because the key has been allocated by BoringSSL and isn't
        // used after this.
        unsafe { EC_KEY_free(self.0) }
    }
}

/// A u8 vector that is zeroed when dropped.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct ZVec(Vec<u8>);

impl ZVec {
    /// Extracts a slice containing the entire vector.
    pub fn as_slice(&self) -> &[u8] {
        &self.0[..]
    }
}

impl From<Vec<u8>> for ZVec {
    fn from(v: Vec<u8>) -> Self {
        Self(v)
    }
}

struct Cbb(CBB);

impl Cbb {
    fn new(initial_capacity: usize) -> Result<Self> {
        let mut cbb = MaybeUninit::uninit();
        // SAFETY: Initializes the CBB. The return is checked below.
        let ret = unsafe { CBB_init(cbb.as_mut_ptr(), initial_capacity) };
        check_result(ret, BoringSSLApiName::CBB_init)?;
        // SAFETY: The CBB object should be initialized since `CBB_init` succeeds.
        Ok(Self(unsafe { cbb.assume_init() }))
    }

    fn finish(mut self) -> Result<ZVec> {
        let mut out_data = ptr::null_mut();
        let mut out_len = 0;
        // SAFETY: This is safe because the CBB pointer is initialized with `CBB_init()` at the
        // creation.
        let ret = unsafe { CBB_finish(self.as_mut(), &mut out_data, &mut out_len) };
        check_result(ret, BoringSSLApiName::CBB_finish)?;
        // It is legal for BoringSSL to return null pointer and 0 for `out_len` if `CBB` was
        // initialized with zero capacity and nothing was written to it.
        if out_data.is_null() {
            return Ok(Vec::new().into());
        }
        // SAFETY: The pointer is non-null and the buffer is allocated by `OPENSSL_malloc`
        // and is supposed to be valid for `out_len` bytes.
        let buf = unsafe { slice::from_raw_parts(out_data, out_len) };
        let buf = buf.to_vec().into();
        // SAFETY: It is safe because the buffer has been allocated by `OPENSSL_malloc` and the
        // data of the buffer has been copied to a vector.
        unsafe {
            OPENSSL_free(out_data as *mut _);
        }
        Ok(buf)
    }
}

impl AsMut<CBB> for Cbb {
    fn as_mut(&mut self) -> &mut CBB {
        &mut self.0
    }
}

impl Drop for Cbb {
    fn drop(&mut self) {
        // SAFETY: This is safe because the CBB pointer is initialized with `CBB_init()` at the
        // creation.
        unsafe { CBB_cleanup(self.as_mut()) }
    }
}

// TODO(b/301068421): Unit tests the EcKey.
