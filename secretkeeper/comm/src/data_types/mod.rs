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

//! Implements the data structures specified by SecretManagement.cddl in Secretkeeper HAL.
//!  Data structures specified by SecretManagement.cddl in Secretkeeper HAL.
//!  Note this library must stay in sync with:
//!      platform/hardware/interfaces/security/\
//!      secretkeeper/aidl/android/hardware/security/secretkeeper/SecretManagement.cddl

pub mod cbor_ser;
pub mod error;
pub mod packet;
pub mod request;
pub mod request_response_impl;
pub mod response;
use crate::data_types::cbor_ser::{CborBytesConversion, ValueConversion};
use crate::data_types::error::Error;
use alloc::boxed::Box;
use authgraph_derive::AsCborValue;
use ciborium::Value;
use wire::cbor::AsCborValue;
use zeroize::ZeroizeOnDrop;

/// Size of the `id` bstr in SecretManagement.cddl
pub const ID_SIZE: usize = 64;
/// Size of the `secret` bstr in SecretManagement.cddl
pub const SECRET_SIZE: usize = 32;

/// Identifier of Secret. See `id` in SecretManagement.cddl
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Id(pub Box<[u8; ID_SIZE]>);
impl ValueConversion for Id {
    fn from_cbor_value(value: Value) -> Result<Self, Error> {
        Ok(Self(value.into_bytes()?.try_into().map_err(|_| Error::ConversionError)?))
    }

    fn to_cbor_value(self) -> Value {
        Value::Bytes(self.0.to_vec())
    }
}
impl CborBytesConversion for Id {}

/// Secret - corresponds to `secret` in SecretManagement.cddl
// Note This is sensitive data. Do not log!
#[derive(Clone, Eq, PartialEq, ZeroizeOnDrop, AsCborValue)]
pub struct Secret(pub Box<[u8; SECRET_SIZE]>); // TODO: Implement ZeroOnDrop
