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

//! Helpers for using BoringSSL BIGNUM objects.

use crate::util::{check_int_result, to_call_failed_error};
use bssl_avf_error::{ApiName, Error, Result};
use bssl_ffi::{BN_bn2bin_padded, BN_clear_free, BN_new, BIGNUM};
use core::ptr::NonNull;
use core::result;

/// Wrapper of an `BIGNUM` object
pub struct BigNum(NonNull<BIGNUM>);

impl Drop for BigNum {
    fn drop(&mut self) {
        // SAFETY: The pointer has been created with `BN_new`.
        unsafe { BN_clear_free(self.as_mut_ptr()) }
    }
}

impl BigNum {
    /// Creates a new, allocated BIGNUM and initialises it.
    pub fn new() -> Result<Self> {
        // SAFETY: The returned pointer is checked below.
        let bn = unsafe { BN_new() };
        NonNull::new(bn).map(Self).ok_or(to_call_failed_error(ApiName::BN_new))
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut BIGNUM {
        self.0.as_ptr()
    }
}

/// Converts the `BigNum` to a big-endian integer. The integer is padded with leading zeros up to
/// size `N`. The conversion fails if `N` is smaller thanthe size of the integer.
impl<const N: usize> TryFrom<BigNum> for [u8; N] {
    type Error = Error;

    fn try_from(bn: BigNum) -> result::Result<Self, Self::Error> {
        let mut num = [0u8; N];
        // SAFETY: The `BIGNUM` pointer has been created with `BN_new`.
        let ret = unsafe { BN_bn2bin_padded(num.as_mut_ptr(), num.len(), bn.0.as_ptr()) };
        check_int_result(ret, ApiName::BN_bn2bin_padded)?;
        Ok(num)
    }
}
