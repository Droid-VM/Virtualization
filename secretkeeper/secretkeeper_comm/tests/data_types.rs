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

//! Unit tests for testing serialization & deserialization of exported data_types.

use secretkeeper_comm::data_types::error::SecretkeeperError;
use secretkeeper_comm::data_types::request::Request;
use secretkeeper_comm::data_types::request_response_impl::{GetVersionRequest, GetVersionResponse};
use secretkeeper_comm::data_types::response::Response;

#[cfg(test)]
rdroidtest::test_main!();

#[cfg(test)]
mod tests {
    use super::*;
    use rdroidtest::test;

    test!(request_serialization_deserialization);
    fn request_serialization_deserialization() {
        let req = GetVersionRequest {};
        let packet = req.serialize_to_packet().unwrap();
        assert_eq!(req.get_opcode(), packet.get_opcode().unwrap());
        let req_deserialized = *GetVersionRequest::deserialize_from_packet(packet).unwrap();
        assert_eq!(req, req_deserialized);
    }

    test!(success_response_serialization_deserialization);
    fn success_response_serialization_deserialization() {
        let response = GetVersionResponse { version: 1 };
        let packet = response.serialize_to_packet().unwrap();
        assert!(!packet.is_error().unwrap());
        let response_deserialized = *GetVersionResponse::deserialize_from_packet(packet).unwrap();
        assert_eq!(response, response_deserialized);
    }

    test!(error_response_serialization_deserialization);
    fn error_response_serialization_deserialization() {
        let response = SecretkeeperError::RequestMalformed;
        let packet = response.serialize_to_packet().unwrap();
        assert!(packet.is_error().unwrap());
        let response_deserialized = *SecretkeeperError::deserialize_from_packet(packet).unwrap();
        assert_eq!(response, response_deserialized);
    }
}
