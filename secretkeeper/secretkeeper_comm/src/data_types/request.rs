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
use crate::data_types::packet::RequestPacket;
use crate::data_types::request_response_impl::Opcode;
use crate::util::value_to_integer;
use ciborium::Value;
use num_traits::FromPrimitive;

/// Collection of methods defined for Secretkeeper's request-like data structures,
/// e.g. GetVersionRequestPacket in the HAL spec.
pub trait Request {
    /// Constructor of the Request object. The implementation of this constructor should verify
    /// the opcode is as expected by HAL spec.
    ///
    /// # Arguments
    /// * `opcode` - Each Request type is associated with an opcode. See `Opcode` in
    ///   SecretManagement.cddl. The implementation of this constructor should verify the opcode is
    ///   as expected by HAL spec.
    /// * `args` - The vector of arguments associated with this request. Each argument is a ciborium
    ///   `Value` type.
    fn init(opcode: Opcode, args: Vec<Value>) -> Result<Box<Self>, InternalError>;

    /// Get the opcode of the request.
    fn get_opcode(&self) -> Opcode;

    /// Get the 'arguments' of this request. Each argument is a ciborium `Value` type.
    fn get_args(&self) -> Vec<Value>;

    /// Serialize the request to a `RequestPacket`. Layers below functional layer such as crypto
    /// work with RequestPacket & are not aware of the 'content' of the RequestPacket.
    /// `RequestPacket`, as per SecretManagement.cddl is:
    ///      RequestPacket<Opcode, Params> = [
    ///         Opcode,
    ///         Params
    ///      ]
    fn serialize_to_packet(&self) -> Result<RequestPacket, InternalError> {
        let mut res = self.get_args();
        res.insert(0, Value::from(self.get_opcode() as u16));
        Ok(RequestPacket(Value::Array(res)))
    }

    /// Construct the request struct from given RequestPacket.
    fn deserialize_from_packet(packet: RequestPacket) -> Result<Box<Self>, InternalError> {
        let mut req = packet.0.into_array().map_err(|_| InternalError::ConversionError)?;
        if req.is_empty() {
            return Err(InternalError::RequestMalformed);
        }
        let num: u16 = value_to_integer(&req[0])?.try_into()?;
        let opcode = Opcode::from_u16(num).ok_or(InternalError::RequestMalformed)?;
        req.remove(0);
        Self::init(opcode, req)
    }
}
