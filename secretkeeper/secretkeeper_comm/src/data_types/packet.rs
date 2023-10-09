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

/// Encapsulate the data passed between the functional layer and the layer below (crypto) for
/// Request.
#[derive(Debug, PartialEq)]
pub struct RequestPacket(pub Vec<Value>);

impl RequestPacket {
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
    pub fn serialize_to_bytes(&self) -> Result<Vec<u8>, InternalError> {
        value_to_bytes(&Value::Array(self.0.clone()))
    }

    /// Deserialize the bytes into ResponsePacket
    pub fn deserialize_from_bytes(bytes: &[u8]) -> Result<Self, InternalError> {
        Ok(RequestPacket(value_from_bytes(bytes)?.into_array()?))
    }
}

/// Encapsulate the data passed between the functional layer and the layer below (crypto) for
/// Response.
#[derive(Debug, PartialEq)]
pub struct ResponsePacket(pub Vec<Value>);

impl ResponsePacket {
    /// Find if the packet correspond to an error like response.
    pub fn is_error(&self) -> Result<bool, InternalError> {
        if self.0.is_empty() {
            return Err(InternalError::ResponseMalformed);
        }
        let error_code: u16 = value_to_integer(&self.0[0])?.try_into()?;
        Ok(error_code != ERROR_OK)
    }

    /// Serialize the ResponsePacket to bytes
    pub fn serialize_to_bytes(&self) -> Result<Vec<u8>, InternalError> {
        value_to_bytes(&Value::Array(self.0.clone()))
    }

    /// Deserialize the bytes into ResponsePacket
    pub fn deserialize_from_bytes(bytes: &[u8]) -> Result<Self, InternalError> {
        Ok(ResponsePacket(value_from_bytes(bytes)?.into_array()?))
    }
}
