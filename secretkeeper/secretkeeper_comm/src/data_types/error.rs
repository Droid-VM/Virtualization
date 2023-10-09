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

use crate::data_types::response::Response;
use crate::util::value_to_integer;
pub use ciborium::Value;
use core::fmt;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

/// 'Error code' corresponding to successful response.
pub const ERROR_OK: u16 = 0; // All 'real' errors must have non-zero error_codes

/// Errors from Secretkeeper api.
#[derive(Clone, Copy, Debug, Eq, FromPrimitive, PartialEq)]
#[repr(u16)]
pub enum SecretkeeperError {
    /// Request was Malformed.
    RequestMalformed = 1,
    /// Unexpected error in server.
    UnexpectedServerError,
    // TODO(b/291228655): Add other errors such as DicePolicyError.
}

impl fmt::Display for SecretkeeperError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::RequestMalformed => write!(f, "Request was malformed"),
            Self::UnexpectedServerError => write!(f, "Unexpected Error in server"),
        }
    }
}

// SecretkeeperError is valid set of errors from Secretkeeper & are encapsulated in Response
// For more information see `ErrorCode` in SecretManagement.cddl alongside ISecretkeeper.aidl
impl Response for SecretkeeperError {
    fn init(response_cbor: Vec<Value>) -> Result<Box<Self>, InternalError> {
        // TODO(b/291228655): This method currently discards the second value in response_cbor,
        // which contains additional human-readable context in error. Include it!
        if response_cbor.is_empty() || response_cbor.len() > 2 {
            return Err(InternalError::ResponseMalformed);
        }
        let error_code: u16 = value_to_integer(&response_cbor[0])?.try_into()?;
        SecretkeeperError::from_u16(error_code)
            .map_or_else(|| Err(InternalError::ResponseMalformed), |sk_err| Ok(Box::new(sk_err)))
    }

    fn get_error_code(&self) -> u16 {
        *self as u16
    }
}

/// Errors thrown internally by the library.
#[derive(Debug)]
pub enum InternalError {
    /// Request was malformed.
    RequestMalformed,
    /// Response received from the server was malformed.
    ResponseMalformed,
    /// An error happened when serializing to/from a `Value`.
    CborValueError(ciborium::value::Error),
    /// An error happened while casting a type to different type,
    /// including one `Value` type to another.
    ConversionError,
    /// These are rare errors such as failure to allocate heap memory
    UnexpectedError,
}

impl fmt::Display for InternalError {
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
        }
    }
}

impl From<ciborium::value::Error> for InternalError {
    fn from(e: ciborium::value::Error) -> Self {
        Self::CborValueError(e)
    }
}

impl From<ciborium::Value> for InternalError {
    fn from(_e: ciborium::Value) -> Self {
        Self::ConversionError
    }
}

impl From<std::num::TryFromIntError> for InternalError {
    fn from(_e: std::num::TryFromIntError) -> Self {
        Self::ConversionError
    }
}
