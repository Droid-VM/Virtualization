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

use ciborium::Value;
use secretkeeper_comm::data_types::cbor_ser::CborBytesConversion;
use secretkeeper_comm::data_types::error::{Error, SecretkeeperError, ERROR_OK};
use secretkeeper_comm::data_types::packet::{RequestPacket, ResponsePacket, ResponseType};
use secretkeeper_comm::data_types::request::Request;
use secretkeeper_comm::data_types::request_response_impl::Opcode;
use secretkeeper_comm::data_types::request_response_impl::{
    GetSecretRequest, GetSecretResponse, GetVersionRequest, GetVersionResponse, StoreSecretRequest,
    StoreSecretResponse,
};
use secretkeeper_comm::data_types::response::Response;
use secretkeeper_comm::data_types::{Id, Secret};

#[cfg(test)]
rdroidtest::test_main!();

#[cfg(test)]
mod tests {
    use super::*;
    use rdroidtest::test;

    test!(request_serialization_deserialization_get_version);
    fn request_serialization_deserialization_get_version() {
        let _: GetVersionRequest =
            verify_request_structure(GetVersionRequest {}, Opcode::GetVersion);
    }

    test!(request_serialization_deserialization_store_secret);
    fn request_serialization_deserialization_store_secret() {
        let req = StoreSecretRequest::new(ex_id(), ex_secret(), ex_dice_policy());
        let req_other_end: StoreSecretRequest =
            verify_request_structure(req.clone(), Opcode::StoreSecret);
        assert_eq!(req.id().0, req_other_end.id().0);
        assert_eq!(req.secret().0, req_other_end.secret().0);
        assert_eq!(req.sealing_policy(), req_other_end.sealing_policy());
    }

    test!(request_serialization_deserialization_get_secret);
    fn request_serialization_deserialization_get_secret() {
        let req = GetSecretRequest::new(ex_id(), Some(ex_dice_policy()));
        let req_other_end = verify_request_structure(req.clone(), Opcode::GetSecret);
        assert_eq!(req.id().0, req_other_end.id().0);
        assert_eq!(req.updated_sealing_policy(), req_other_end.updated_sealing_policy());

        let req = GetSecretRequest::new(ex_id(), None);
        let req_other_end = verify_request_structure(req.clone(), Opcode::GetSecret);
        assert_eq!(req.id().0, req_other_end.id().0);
        assert_eq!(req.updated_sealing_policy(), req_other_end.updated_sealing_policy());
    }

    test!(success_response_serialization_deserialization_get_version);
    fn success_response_serialization_deserialization_get_version() {
        let response = GetVersionResponse::new(1);
        let res_other_end = verify_response_structure(response.clone(), ResponseType::Success);
        assert_eq!(response.version(), res_other_end.version());
    }

    test!(success_response_serialization_deserialization_store_secret);
    fn success_response_serialization_deserialization_store_secret() {
        let response = StoreSecretResponse {};
        let _: StoreSecretResponse =
            verify_response_structure(response.clone(), ResponseType::Success);
    }

    test!(success_response_serialization_deserialization_get_secret);
    fn success_response_serialization_deserialization_get_secret() {
        let response = GetSecretResponse::new(ex_secret());
        let res_other_end = verify_response_structure(response.clone(), ResponseType::Success);
        assert_eq!(response.secret().0, res_other_end.secret().0);
    }

    test!(error_response_serialization_deserialization);
    fn error_response_serialization_deserialization() {
        let response = SecretkeeperError::RequestMalformed;
        let _: SecretkeeperError = verify_response_structure(response, ResponseType::Error);
    }

    fn verify_request_structure<R: Request>(req: R, expected_opcode: Opcode) -> R {
        let packet = req.serialize_to_packet();
        assert_eq!(packet.opcode().unwrap(), expected_opcode);
        let packet = packet.to_vec().unwrap();
        let packet = RequestPacket::from_slice(&packet).unwrap();
        let req_other_end = *R::deserialize_from_packet(packet).unwrap();
        assert_eq!(req.args(), req_other_end.args());
        req_other_end
    }

    fn verify_response_structure<R: Response>(
        response: R,
        expected_response_type: ResponseType,
    ) -> R {
        let packet = response.serialize_to_packet();
        assert_eq!(packet.response_type().unwrap(), expected_response_type);
        let packet_bytes = packet.to_vec().unwrap();
        let packet = ResponsePacket::from_slice(&packet_bytes).unwrap();
        let response_other_end = *R::deserialize_from_packet(packet).unwrap();
        assert_eq!(response.result(), response_other_end.result());
        response_other_end
    }

    test!(request_creation);
    fn request_creation() {
        // GetVersionRequest: This verifies that the request is indeed GetVersionRequest type.
        let _: GetVersionRequest = *Request::new(vec![]).unwrap();

        // StoreSecretRequest
        let req: StoreSecretRequest = *Request::new(vec![
            Value::Bytes(ex_id_bytes()),
            Value::Bytes(ex_secret_bytes()),
            Value::Bytes(ex_dice_policy()),
        ])
        .unwrap();
        assert_eq!(req.id().0, ex_id().0);
        assert_eq!(req.secret().0, ex_secret().0);
        assert_eq!(req.sealing_policy(), &ex_dice_policy());

        // GetSecretRequest with an updated_sealing_policy
        let req: GetSecretRequest =
            *Request::new(vec![Value::Bytes(ex_id_bytes()), Value::Bytes(ex_dice_policy())])
                .unwrap();
        assert_eq!(req.id().0, ex_id().0);
        assert_eq!(*req.updated_sealing_policy(), Some(ex_dice_policy()));

        // GetSecretRequest with no updated_sealing_policy
        let req: GetSecretRequest =
            *Request::new(vec![Value::Bytes(ex_id_bytes()), Value::Null]).unwrap();
        assert_eq!(req.id().0, ex_id().0);
        assert_eq!(*req.updated_sealing_policy(), None);
    }

    test!(response_creation);
    fn response_creation() {
        // GetVersionResponse
        let res: GetVersionResponse =
            *Response::new(vec![Value::from(ERROR_OK), Value::from(5)]).unwrap();
        assert_eq!(res.version(), 5);

        // StoreSecretResponse
        let _ = *<StoreSecretResponse as Response>::new(vec![Value::from(ERROR_OK)]).unwrap();

        // GetSecretResponse
        let res = *<GetSecretResponse as Response>::new(vec![
            Value::from(ERROR_OK),
            Value::Bytes(ex_secret_bytes()),
        ])
        .unwrap();
        assert_eq!(res.secret().0, ex_secret().0);
    }

    test!(invalid_request_creation);
    fn invalid_request_creation() {
        // A GetVersionRequest with non-zero arg is invalid.
        assert!(<GetVersionRequest as Request>::new(vec![Value::Null]).is_err());

        // StoreSecretRequest
        // Incorrect number of arg is invalid.
        assert!(<StoreSecretRequest as Request>::new(vec![]).is_err(),);
        // Incorrect arg type is invalid.
        assert!(<StoreSecretRequest as Request>::new(vec![
            Value::Bytes(ex_id_bytes()),
            Value::Integer(22.into()),
            Value::Bytes(ex_dice_policy()),
        ])
        .is_err());

        // A GetSecretRequest
        // Incorrect number of arg is invalid.
        assert!(<GetSecretRequest as Request>::new(vec![]).is_err());
        // Incorrect arg type is invalid.
        assert!(<GetSecretRequest as Request>::new(vec![
            Value::Integer(22.into()),
            Value::Bytes(ex_dice_policy()),
        ])
        .is_err());
    }

    test!(invalid_response_creation_get_version);
    fn invalid_response_creation_get_version() {
        // A response with non-zero error_code is an invalid success response.
        assert!(<GetVersionResponse as Response>::new(vec![
            Value::from(SecretkeeperError::RequestMalformed as u16),
            Value::from(5)
        ])
        .is_err());

        // A response with incorrect size of array is invalid.
        assert!(<GetVersionResponse as Response>::new(vec![
            Value::from(ERROR_OK),
            Value::from(5),
            Value::from(7)
        ])
        .is_err());

        // A response with incorrect type is invalid.
        assert!(<GetVersionResponse as Response>::new(vec![
            Value::from(ERROR_OK),
            Value::from("a tstr")
        ])
        .is_err());
    }

    test!(invalid_store_secret_response_creation);
    fn invalid_store_secret_response_creation() {
        // A response with non-zero error_code is an invalid success response.
        assert!(<StoreSecretResponse as Response>::new(vec![Value::from(
            SecretkeeperError::RequestMalformed as u16
        ),])
        .is_err());

        // A response with incorrect type or size of array is invalid.
        assert!(<StoreSecretResponse as Response>::new(vec![
            Value::from(ERROR_OK),
            Value::from(7)
        ])
        .is_err());
    }

    test!(invalid_get_secret_response_creation);
    fn invalid_get_secret_response_creation() {
        // A response with non-zero error_code is an invalid success response.
        assert!(<GetSecretResponse as Response>::new(vec![
            Value::from(SecretkeeperError::RequestMalformed as u16),
            Value::Bytes(ex_secret_bytes()),
        ])
        .is_err());

        // A response with incorrect type or size of array is invalid.
        assert!(<GetSecretResponse as Response>::new(vec![Value::from(ERROR_OK), Value::from(7)])
            .is_err());
    }

    test!(invalid_error_response_creation);
    fn invalid_error_response_creation() {
        // A response with ERROR_OK(0) as the error_code is an invalid error response.
        assert_eq!(
            <SecretkeeperError as Response>::new(vec![Value::from(ERROR_OK)]).unwrap_err(),
            Error::ResponseMalformed
        );
    }

    fn ex_id_bytes() -> Vec<u8> {
        (b"sixty_four_bytes_in_a_sentences_can_make_it_really_really_longer").to_vec()
    }

    fn ex_secret_bytes() -> Vec<u8> {
        (*b"thirty_two_bytes_long_sentences_").to_vec()
    }

    fn ex_id() -> Id {
        Id(Box::new(ex_id_bytes().try_into().unwrap()))
    }

    fn ex_secret() -> Secret {
        Secret(Box::new(ex_secret_bytes().try_into().unwrap()))
    }

    fn ex_dice_policy() -> Vec<u8> {
        (*b"example_string").to_vec()
    }
}
