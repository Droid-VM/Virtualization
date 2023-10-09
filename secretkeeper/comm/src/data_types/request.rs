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

use crate::cbor_convert::value_to_integer;
use crate::data_types::error::Error;
use crate::data_types::packet::RequestPacket;
use crate::data_types::request_response_impl::Opcode;
use ciborium::Value;
use num_traits::FromPrimitive;

/// Collection of methods defined for Secretkeeper's request-like data structures,
/// e.g. GetVersionRequestPacket in the HAL spec.
///
/// Keep in sync with SecretManagement.cddl, in particular `RequestPacket` type.
pub trait Request {
    /// Opcode of the request: Each Request type is associated with an opcode. See `Opcode` in
    /// SecretManagement.cddl.
    const OPCODE: Opcode;

    /// Constructor of the Request object. The implementation of this constructor should check
    /// the args' type is adhered to the HAL spec.
    ///
    /// # Arguments
    /// * `args` - The vector of arguments associated with this request. Each argument is a
    ///   `ciborium::Value` type. See `Params` in `RequestPacket` in SecretManagement.cddl
    fn new(args: Vec<Value>) -> Result<Box<Self>, Error>;

    /// Get the 'arguments' of this request.
    fn args(&self) -> Vec<Value>;

    /// Serialize the request to a [`RequestPacket`]. Layers below functional layer
    /// work with RequestPacket & are not aware of the concrete Request.
    /// [`RequestPacket`], as per SecretManagement.cddl is:
    ///      RequestPacket<Opcode, Params> = [
    ///         Opcode,
    ///         Params
    ///      ]
    fn serialize_to_packet(&self) -> RequestPacket {
        let mut res = self.args();
        res.insert(0, Value::from(Self::OPCODE as u16));
        RequestPacket::from(res)
    }

    /// Construct the Request struct from given RequestPacket.
    fn deserialize_from_packet(packet: RequestPacket) -> Result<Box<Self>, Error> {
        let mut req = packet.into_inner();
        if req.is_empty() {
            return Err(Error::RequestMalformed);
        }
        let num: u16 = value_to_integer(&req[0])?.try_into()?;
        let opcode_packet = Opcode::from_u16(num).ok_or(Error::RequestMalformed)?;
        if opcode_packet != Self::OPCODE {
            return Err(Error::RequestMalformed);
        }
        req.remove(0);
        Self::new(req)
    }
}
