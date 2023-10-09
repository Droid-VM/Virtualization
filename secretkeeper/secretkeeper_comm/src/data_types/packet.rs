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
use crate::util::value_to_integer;
use num_traits::FromPrimitive;

/// Encapsulate the data passed between the functional layer and the layer below (crypto) for
/// Request.
#[derive(Debug, PartialEq)]
pub struct RequestPacket(pub Value);

impl RequestPacket {
    /// Extract opcode corresponding to this packet. As defined in the cddl, this is
    /// the first value in the CBOR array.
    pub fn get_opcode(&self) -> Result<Opcode, InternalError> {
        let arr = self.0.clone().into_array()?;
        if arr.is_empty() {
            return Err(InternalError::RequestMalformed);
        }
        let num: u16 = value_to_integer(&arr[0])?.try_into()?;

        Opcode::from_u16(num).ok_or(InternalError::RequestMalformed)
    }

    /// Serialize the ResponsePacket to bytes
    pub fn serialize_to_bytes(&self) -> Result<Vec<u8>, InternalError> {
        value_to_bytes(&self.0)
    }

    /// Deserialize the bytes into ResponsePacket
    pub fn deserialize_from_bytes(bytes: &[u8]) -> Result<Self, InternalError> {
        Ok(RequestPacket(value_from_bytes(bytes)?))
    }
}

/// Encapsulate the data passed between the functional layer and the layer below (crypto) for
/// Response.
#[derive(Debug, PartialEq)]
pub struct ResponsePacket(pub Value);

impl ResponsePacket {
    /// Find if the packet correspond to an error like response.
    pub fn is_error(&self) -> Result<bool, InternalError> {
        let arr = self.0.clone().into_array()?;
        if arr.is_empty() {
            return Err(InternalError::ResponseMalformed);
        }
        let error_code: u16 = value_to_integer(&arr[0])?.try_into()?;
        Ok(error_code != ERROR_OK)
    }

    /// Serialize the ResponsePacket to bytes
    pub fn serialize_to_bytes(&self) -> Result<Vec<u8>, InternalError> {
        value_to_bytes(&self.0)
    }

    /// Deserialize the bytes into ResponsePacket
    pub fn deserialize_from_bytes(bytes: &[u8]) -> Result<Self, InternalError> {
        Ok(ResponsePacket(value_from_bytes(bytes)?))
    }
}

/// Decodes the provided binary CBOR-encoded value and returns a
/// ciborium::Value struct wrapped in Result.
fn value_from_bytes(mut bytes: &[u8]) -> Result<Value, InternalError> {
    let value =
        ciborium::de::from_reader(&mut bytes).map_err(|_| InternalError::ConversionError)?;
    // Ciborium tries to read one Value, but doesn't care if there is trailing data after it. We do
    if !bytes.is_empty() {
        return Err(InternalError::ConversionError);
    }
    Ok(value)
}

/// Encodes a ciborium::Value into bytes.
fn value_to_bytes(value: &Value) -> Result<Vec<u8>, InternalError> {
    let mut bytes: Vec<u8> = Vec::new();
    ciborium::ser::into_writer(&value, &mut bytes).map_err(|_| InternalError::UnexpectedError)?;
    Ok(bytes)
}
