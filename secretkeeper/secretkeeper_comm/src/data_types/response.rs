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

use crate::data_types::error::InternalError;
use crate::data_types::packet::ResponsePacket;
use ciborium::Value;

/// Collection of methods defined for Secretkeeper's response-like data structures,
/// e.g. GetVersionResponsePacket. Note - response can be errors like `RequestMalformed`
pub trait Response {
    /// Constructor of the Response object.
    ///
    /// # Argument - A vector of ciborium Value such that:
    ///     For success-like responses:
    ///         ResponsePacketSuccess = [
    ///             0,                          ; Indicates successful Response
    ///             result : Result
    ///         ]
    ///     For error responses:
    ///         ResponsePacketError = [
    ///             error_code: ErrorCode,      ; Indicate the error
    ///             error_message: tstr         ; Additional human-readable context
    ///         ]
    /// See ResponsePacket<Result> in SecretManagement.cddl alongside ISecretkeeper.aidl
    fn init(response_cbor: Vec<Value>) -> Result<Box<Self>, InternalError>;

    /// The result in the `Response`. By default this is empty, but Response structures like
    /// GetVersionResponse must overwrite these to return the expected non-empty result.
    fn get_result(&self) -> Vec<Value> {
        Vec::new()
    }

    /// Error code corresponding to the response. The default value is 0 but that will work only
    /// for successful responses. Error-like response structures must overwrite this method.
    fn get_error_code(&self) -> u16 {
        0 // Indicates success
    }

    /// Serialize the response to a `ResponsePacket`. Layers below functional layer such as crypto
    /// work with ResponsePacket & are not aware of the 'content' of the ResponsePacket.
    fn serialize_to_packet(&self) -> Result<ResponsePacket, InternalError> {
        let mut res = self.get_result();
        res.insert(0, Value::from(self.get_error_code()));
        Ok(ResponsePacket(Value::Array(res)))
    }

    /// Construct the response struct from given ResponsePacket.
    fn deserialize_from_packet(packet: ResponsePacket) -> Result<Box<Self>, InternalError> {
        let res = packet.0.into_array().map_err(|_| InternalError::ConversionError)?;
        // There is no empty response packet really, all responses in Secretkeeper HAL at least
        // have error_code or '0'; so throw an error!
        if res.is_empty() {
            return Err(InternalError::ResponseMalformed);
        }
        Self::init(res)
    }
}
