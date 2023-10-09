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

pub use ciborium::Value;

use crate::data_types::error::InternalError;
use crate::data_types::error::ERROR_OK;
use crate::data_types::request_response_impl::Opcode;
use crate::util::{value_from_bytes, value_to_bytes, value_to_integer};
use num_traits::FromPrimitive;

/// Encapsulate Request-like data that functional layer operates on. All structures
/// that implements crate::data_types::request::Request can be serialized to ResponsePacket.
/// Similarly all RequestPacket can be deserialized to concrete Requests.
/// Keep in sync with HAL spec (in particular RequestPacket):
///     security/secretkeeper/aidl/android/hardware/security/secretkeeper/SecretManagement.cddl
#[derive(Clone, Debug, PartialEq)]
pub struct RequestPacket(Vec<Value>);

impl RequestPacket {
    /// Construct a RequestPacket from array of [`ciborium::Value`]
    pub fn new(request_cbor: Vec<Value>) -> Self {
        Self(request_cbor)
    }

    /// Get the containing cbor. This can be used for getting concrete response objects.
    /// Keep in sync with [`crate::data_types::request::Request::serialize_to_packet()`]
    pub fn get_content(self) -> Vec<Value> {
        self.0
    }

    /// Extract opcode corresponding to this packet. As defined in the cddl, this is
    /// the first value in the CBOR array.
    pub fn get_opcode(&self) -> Result<Opcode, InternalError> {
        if self.0.is_empty() {
            return Err(InternalError::RequestMalformed);
        }
        let num: u16 = value_to_integer(&self.0[0])?.try_into()?;

        Opcode::from_u16(num).ok_or(InternalError::RequestMalformed)
    }

    /// Serialize the ResponsePacket to bytes
    pub fn serialize_to_bytes(self) -> Result<Vec<u8>, InternalError> {
        value_to_bytes(&Value::Array(self.0))
    }

    /// Deserialize the bytes into ResponsePacket
    pub fn deserialize_from_bytes(bytes: &[u8]) -> Result<Self, InternalError> {
        Ok(RequestPacket(value_from_bytes(bytes)?.into_array()?))
    }
}

/// Encapsulate Response like data that the functional layer operates on. All structures
/// that implements crate::data_types::response::Response can be serialized to ResponsePacket.
/// Similarly all ResponsePacket can be deserialized to concrete Response.
#[derive(Clone, Debug, PartialEq)]
pub struct ResponsePacket(Vec<Value>);

impl ResponsePacket {
    /// Construct a ResponsePacket from array of [`ciborium::Value`]
    pub fn new(response_cbor: Vec<Value>) -> Self {
        Self(response_cbor)
    }

    /// Get raw content. This can be used for getting concrete response objects.
    /// Keep in sync with crate::data_types::response::Response::serialize_to_packet()
    pub fn get_content(self) -> Vec<Value> {
        self.0
    }

    /// Find if the packet correspond to an error like response.
    pub fn is_error(&self) -> Result<bool, InternalError> {
        if self.0.is_empty() {
            return Err(InternalError::ResponseMalformed);
        }
        let error_code: u16 = value_to_integer(&self.0[0])?.try_into()?;
        Ok(error_code != ERROR_OK)
    }

    /// Serialize the ResponsePacket to bytes
    pub fn serialize_to_bytes(self) -> Result<Vec<u8>, InternalError> {
        value_to_bytes(&Value::Array(self.0))
    }

    /// Deserialize the bytes into ResponsePacket
    pub fn deserialize_from_bytes(bytes: &[u8]) -> Result<Self, InternalError> {
        Ok(ResponsePacket(value_from_bytes(bytes)?.into_array()?))
    }
}
