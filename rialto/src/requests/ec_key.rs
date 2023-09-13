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

use alloc::vec;
use alloc::vec::Vec;
use bssl_ffi::BN_bn2bin;
use bssl_ffi::BN_clear_free;
use bssl_ffi::BN_new;
use bssl_ffi::BN_num_bytes;
use bssl_ffi::CBB_cleanup;
use bssl_ffi::CBB_finish;
use bssl_ffi::CBB_init;
use bssl_ffi::EC_KEY_check_key;
use bssl_ffi::EC_KEY_free;
use bssl_ffi::EC_KEY_generate_key;
use bssl_ffi::EC_KEY_get0_group;
use bssl_ffi::EC_KEY_get0_public_key;
use bssl_ffi::EC_KEY_marshal_private_key;
use bssl_ffi::EC_KEY_new_by_curve_name;
use bssl_ffi::EC_POINT_get_affine_coordinates;
use bssl_ffi::NID_X9_62_prime256v1; // EC P-256 CURVE Nid
use bssl_ffi::OPENSSL_free;
use bssl_ffi::BIGNUM;
use bssl_ffi::CBB;
use bssl_ffi::EC_GROUP;
use bssl_ffi::EC_KEY;
use bssl_ffi::EC_POINT;
use core::mem::MaybeUninit;
use core::ptr::{self, NonNull};
use core::result;
use core::slice;
use coset::{iana, CoseKey, CoseKeyBuilder};
use service_vm_comm::{BoringSSLApiName, RequestProcessingError};
use zeroize::{Zeroize, ZeroizeOnDrop};

type Result<T> = result::Result<T, RequestProcessingError>;

/// Wrapper of an `EC_KEY` object, representing a public or private EC key.
pub struct EcKey(*mut EC_KEY);

impl Drop for EcKey {
    fn drop(&mut self) {
        // SAFETY: It is safe because the key has been allocated by BoringSSL and isn't
        // used after this.
        unsafe { EC_KEY_free(self.0) }
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
        let ret = unsafe { EC_KEY_generate_key(self.0) };
        validate_boringssl_return(ret, BoringSSLApiName::EC_KEY_generate_key)
    }

    /// Invokes `EC_KEY_check_key` that performs several checks on keys stored in `EC_KEY`.
    fn check_key(&mut self) -> Result<()> {
        // SAFETY: The non-null pointer is created with `EC_KEY_new_by_curve_name` and should
        // point to a valid `EC_KEY`.
        let ret = unsafe { EC_KEY_check_key(self.0) };
        validate_boringssl_return(ret, BoringSSLApiName::EC_KEY_check_key)
    }

    /// Returns the `CoseKey` for the public key.
    pub fn cose_public_key(&self) -> Result<CoseKey> {
        const ALGO: iana::Algorithm = iana::Algorithm::ES256;
        const CURVE: iana::EllipticCurve = iana::EllipticCurve::P_256;

        let (x, y) = self.public_key_coordinates()?;
        let key = CoseKeyBuilder::new_ec2_pub_key(CURVE, x, y).algorithm(ALGO).build();
        Ok(key)
    }

    /// Returns the x and y coordinates of the public key.
    fn public_key_coordinates(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        let ec_group = self.ec_group()?;
        let ec_point = self.public_key_ec_point()?;
        let x = BigNum::new()?;
        let y = BigNum::new()?;
        let ctx = ptr::null_mut();
        // SAFETY: All the parameters are checked non-null and initialized when needed.
        // The last parameter `ctx` is generated when needed inside the function.
        let ret = unsafe {
            EC_POINT_get_affine_coordinates(ec_group, ec_point, x.as_mut_ptr(), y.as_mut_ptr(), ctx)
        };
        validate_boringssl_return(ret, BoringSSLApiName::EC_POINT_get_affine_coordinates)?;
        Ok((x.bytes()?, y.bytes()?))
    }

    /// Returns a pointer to the public key point inside `EC_KEY`. The memory region pointed
    /// by the pointer is owned by the `EC_KEY`.
    fn public_key_ec_point(&self) -> Result<*const EC_POINT> {
        let ec_point =
           // SAFETY: It is safe since the key pair has been generated and stored in the
           // `EC_KEY` pointer.
           unsafe { EC_KEY_get0_public_key(self.0) };
        if ec_point.is_null() {
            Err(RequestProcessingError::BoringSSLCallFailed(
                BoringSSLApiName::EC_KEY_get0_public_key,
            ))
        } else {
            Ok(ec_point)
        }
    }

    /// Returns a pointer to the `EC_GROUP` object inside `EC_KEY`. The memory region pointed
    /// by the pointer is owned by the `EC_KEY`.
    fn ec_group(&self) -> Result<*const EC_GROUP> {
        let group =
           // SAFETY: It is safe since the key pair has been generated and stored in the
           // `EC_KEY` pointer.
           unsafe { EC_KEY_get0_group(self.0) };
        if group.is_null() {
            Err(RequestProcessingError::BoringSSLCallFailed(BoringSSLApiName::EC_KEY_get0_group))
        } else {
            Ok(group)
        }
    }

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
        validate_boringssl_return(ret, BoringSSLApiName::EC_KEY_marshal_private_key)?;
        cbb.finish()
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

impl Drop for Cbb {
    fn drop(&mut self) {
        // SAFETY: This is safe because the CBB pointer is initialized with `CBB_init()` at the
        // creation.
        unsafe { CBB_cleanup(self.as_mut()) }
    }
}

impl Cbb {
    fn new(initial_capacity: usize) -> Result<Self> {
        let mut cbb = MaybeUninit::uninit();
        // SAFETY: Initializes the CBB. The return is checked below.
        let ret = unsafe { CBB_init(cbb.as_mut_ptr(), initial_capacity) };
        validate_boringssl_return(ret, BoringSSLApiName::CBB_init)?;
        // SAFETY: The CBB object should be initialized since `CBB_init` succeeds.
        Ok(Self(unsafe { cbb.assume_init() }))
    }

    fn finish(mut self) -> Result<ZVec> {
        let mut out_data = ptr::null_mut();
        let mut out_len = 0;
        // SAFETY: This is safe because the CBB pointer is initialized with `CBB_init()` at the
        // creation.
        let ret = unsafe { CBB_finish(self.as_mut(), &mut out_data, &mut out_len) };
        validate_boringssl_return(ret, BoringSSLApiName::CBB_finish)?;
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
        // `OPENSSL_free` also zeroes the buffer of `out_data`.
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

/// Validates the return value of a BoringSSL API call.
fn validate_boringssl_return(ret: i32, api_name: BoringSSLApiName) -> Result<()> {
    if ret == 1 {
        Ok(())
    } else {
        assert_eq!(ret, 0, "Unexpected return value {ret} for {api_name:?}");
        Err(RequestProcessingError::BoringSSLCallFailed(api_name))
    }
}

struct BigNum(NonNull<BIGNUM>);

impl Drop for BigNum {
    fn drop(&mut self) {
        // SAFETY: The pointer has been created with `BN_new`.
        unsafe { BN_clear_free(self.as_mut_ptr()) }
    }
}

impl BigNum {
    fn new() -> Result<Self> {
        // SAFETY: The returned pointer is checked below.
        let bn = unsafe { BN_new() };
        if bn.is_null() {
            Err(RequestProcessingError::BoringSSLCallFailed(BoringSSLApiName::BN_new))
        } else {
            // SAFETY: `bn` has been checked to be non-null.
            let bn = unsafe { NonNull::new_unchecked(bn) };
            Ok(Self(bn))
        }
    }

    fn num_bytes(&self) -> Result<usize> {
        // SAFETY: The pointer has been created with `BN_new`.
        let len = unsafe { BN_num_bytes(self.as_mut_ptr()) };
        len.try_into().map_err(|_| {
            RequestProcessingError::BoringSSLCallFailed(BoringSSLApiName::BN_num_bytes)
        })
    }

    fn bytes(&self) -> Result<Vec<u8>> {
        let len = self.num_bytes()?;
        let mut res = vec![0u8; len];
        // SAFETY: The pointer has been created with `BN_new`.
        let read_len = unsafe { BN_bn2bin(self.as_mut_ptr(), res.as_mut_ptr()) };
        if read_len == len {
            Ok(res)
        } else {
            Err(RequestProcessingError::BoringSSLCallFailed(BoringSSLApiName::BN_bn2bin))
        }
    }

    fn as_mut_ptr(&self) -> *mut BIGNUM {
        self.0.as_ptr()
    }
}

// TODO(b/301068421): Unit tests the EcKey.
