/*
 * Copyright (C) 2023 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Serialization/Deserialization primitives for data types defined in SecretManagement.cddl.
//! SecretManagement HAL is a `CBOR` based protocol. It defines several CBOR based types. See:
//! platform/hardware/interfaces/security/\
//!     secretkeeper/aidl/android/hardware/security/secretkeeper/SecretManagement.cddl

use crate::cbor_convert::{value_from_bytes, value_to_bytes};
use crate::data_types::error::Error;
use alloc::vec::Vec;
use ciborium::Value;

/// Types that can be converted to/from CBOR values.
pub trait ValueConversion: Sized {
    /// Convert the object into `ciborium::Value`
    fn to_cbor_value(self) -> Value;

    /// Get the object from `ciborium::Value`
    fn from_cbor_value(val: Value) -> Result<Self, Error>;
}

/// Types that follow CBOR based encoding/decoding.
pub trait CborBytesConversion: ValueConversion {
    /// Encodes a [`ciborium::Value`] into bytes.
    fn to_vec(self) -> Result<Vec<u8>, Error> {
        value_to_bytes(&self.to_cbor_value())
    }

    /// Decodes the provided binary CBOR-encoded value into concrete type.
    fn from_slice(bytes: &[u8]) -> Result<Self, Error> {
        Self::from_cbor_value(value_from_bytes(bytes)?)
    }
}
