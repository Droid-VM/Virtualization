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

// derive(N) generates a method that is missing a docstring.
#![allow(missing_docs)]

use crate::cbor_convert::value_to_integer;
use crate::data_types::response::Response;
use alloc::boxed::Box;
use alloc::vec::Vec;
pub use ciborium::Value;
use core::fmt;
use enumn::N;

/// 'Error code' corresponding to successful response.
pub const ERROR_OK: u16 = 0; // All real errors must have non-zero error_codes

/// Errors from Secretkeeper api. Keep in sync with `ErrorCode` defined for Secretkeeper HAL
/// at SecretManagement.cddl
#[derive(Clone, Copy, Debug, Eq, N, PartialEq)]
pub enum SecretkeeperError {
    /// Request was Malformed.
    RequestMalformed = 1,
    /// Requested Entry not found.
    EntryNotFound = 2,
    /// Unexpected error in server.
    UnexpectedServerError = 3,
    /// Errors originating in serializer or deserializer
    SerializationError = 4,
    // TODO(b/291228655): Add other errors such as DicePolicyError.
    /// Policy library threw error
    DicePolicyError = 5,
}

impl fmt::Display for SecretkeeperError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::RequestMalformed => write!(f, "Request was malformed"),
            Self::EntryNotFound => write!(f, "Requested Entry not found"),
            Self::UnexpectedServerError => write!(f, "Unexpected Error in server"),
            Self::SerializationError => {
                write!(f, "Errors originating in serializer or deserializer")
            }
            Self::DicePolicyError => write!(f, "Policy library threw error"),
        }
    }
}

// [`SecretkeeperError`] are valid [`Response`] types.
// For more information see `ErrorCode` in SecretManagement.cddl alongside ISecretkeeper.aidl
impl Response for SecretkeeperError {
    fn new(response_cbor: Vec<Value>) -> Result<Box<Self>, Error> {
        // TODO(b/291228655): This method currently discards the second value in response_cbor,
        // which contains additional human-readable context in error. Include it!
        if response_cbor.is_empty() || response_cbor.len() > 2 {
            return Err(Error::ResponseMalformed);
        }
        let error_code: u16 = value_to_integer(&response_cbor[0])?.try_into()?;
        SecretkeeperError::n(error_code)
            .map_or_else(|| Err(Error::ResponseMalformed), |sk_err| Ok(Box::new(sk_err)))
    }

    fn error_code(&self) -> u16 {
        *self as u16
    }
}

/// Errors thrown internally by the library.
#[derive(Debug)]
pub enum Error {
    /// Request was malformed.
    RequestMalformed,
    /// Response received from the server was malformed.
    ResponseMalformed,
    /// An error happened when serializing to/from a [`Value`].
    CborValueError(ciborium::value::Error),
    /// An error happened while casting a type to different type,
    /// including one [`Value`] type to another.
    ConversionError,
    /// These are unexpected errors, which should never really happen.
    UnexpectedError,
    // /// Storage related error
    // StorageError(StorageError),
    /// Policy library threw error, TODO more content please
    DicePolicyError,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::RequestMalformed => write!(f, "Request was malformed"),
            Self::ResponseMalformed => write!(f, "Response was malformed"),
            Self::CborValueError(e) => {
                write!(f, "An error happened when serializing to/from a CBOR Value {:?}", e)
            }
            Self::ConversionError => {
                write!(f, "An error happened while converting one type to another.")
            }
            Self::UnexpectedError => write!(f, "Unexpected error"),
            Self::DicePolicyError => write!(f, "Policy library threw error"),
        }
    }
}

impl From<ciborium::value::Error> for Error {
    fn from(e: ciborium::value::Error) -> Self {
        Self::CborValueError(e)
    }
}

impl From<ciborium::Value> for Error {
    fn from(_e: ciborium::Value) -> Self {
        Self::ConversionError
    }
}

impl From<core::num::TryFromIntError> for Error {
    fn from(_e: core::num::TryFromIntError) -> Self {
        Self::ConversionError
    }
}

// impl From<StorageError> for Error {
//     fn from(e: StorageError) -> Self {
//         Self::StorageError(e)
//     }
// }

/// Errors from storage API, TODO - maybe move to libsecretkeepercore
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageError {
    /// Requested Entry not found.
    EntryNotFound = 1,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::EntryNotFound => write!(f, "Requested Entry not found"),
        }
    }
}
