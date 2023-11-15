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
use secretkeeper_comm::data_types::packet::{RequestPacket, ResponsePacket, ResponseType};
use secretkeeper_comm::data_types::request::Request;
use secretkeeper_comm::data_types::request_response_impl::Opcode;
use secretkeeper_comm::data_types::request_response_impl::{
    GetSecretRequest, GetSecretResponse, GetVersionRequest, GetVersionResponse, StoreSecretRequest,
    StoreSecretResponse,
};
use secretkeeper_comm::data_types::response::Response;

#[cfg(test)]
rdroidtest::test_main!();

#[cfg(test)]
mod tests {
    use super::*;
    use rdroidtest::test;

    test!(request_serialization_deserialization_get_version);
    fn request_serialization_deserialization_get_version() {
        verify_request_structure(GetVersionRequest {}, Opcode::GetVersion);
    }

    test!(request_serialization_deserialization_store_secret);
    fn request_serialization_deserialization_store_secret() {
        let req = StoreSecretRequest::new(
            (*b"sixty_four_bytes_in_a_sentences_can_make_it_really_really_longer").into(),
            (*b"thirty_two_bytes_long_sentences_").into(),
            b"meaningless_for_unit_test".to_vec(),
        );
        verify_request_structure(req, Opcode::StoreSecret);
    }

    test!(request_serialization_deserialization_get_secret);
    fn request_serialization_deserialization_get_secret() {
        let req = GetSecretRequest::new(
            (*b"sixty_four_bytes_in_a_sentences_can_make_it_really_really_longer").into(),
            Some(b"meaningless_for_unit_test".to_vec()),
        );
        verify_request_structure(req, Opcode::GetSecret);
    }

    test!(success_response_serialization_deserialization_get_version);
    fn success_response_serialization_deserialization_get_version() {
        let response = GetVersionResponse::new(1);
        verify_response_structure(response, ResponseType::Success)
    }

    // TODO: create & for responses.
    test!(success_response_serialization_deserialization_store_secret);
    fn success_response_serialization_deserialization_store_secret() {
        let response = StoreSecretResponse {};
        verify_response_structure(response, ResponseType::Success)
    }

    // TODO: create & for responses.
    test!(success_response_serialization_deserialization_get_secret);
    fn success_response_serialization_deserialization_get_secret() {
        let response = GetSecretResponse::new((*b"thirty_two_bytes_long_sentences_").into());
        verify_response_structure(response, ResponseType::Success)
    }

    test!(error_response_serialization_deserialization);
    fn error_response_serialization_deserialization() {
        let response = SecretkeeperError::RequestMalformed;
        verify_response_structure(response, ResponseType::Error);
    }

    fn verify_request_structure<R: Request + core::fmt::Debug + core::cmp::PartialEq>(
        req: R,
        expected_opcode: Opcode,
    ) {
        let packet = req.serialize_to_packet();
        assert_eq!(packet.opcode().unwrap(), expected_opcode);
        assert_eq!(
            RequestPacket::from_bytes(&packet.clone().into_bytes().unwrap()).unwrap(),
            packet
        );
        assert_eq!(req, *R::deserialize_from_packet(packet).unwrap());
    }

    fn verify_response_structure<R: Response + core::fmt::Debug + core::cmp::PartialEq>(
        response: R,
        expected_response_type: ResponseType,
    ) {
        let packet = response.serialize_to_packet();
        assert_eq!(packet.response_type().unwrap(), expected_response_type);
        assert_eq!(
            ResponsePacket::from_bytes(&packet.clone().into_bytes().unwrap()).unwrap(),
            packet
        );
        assert_eq!(response, *R::deserialize_from_packet(packet).unwrap());
    }
}
