// Copyright 2023, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! This module contains functions related to the attestation of the
//! service VM via the RKP (Remote Key Provisioning) server.

use super::ec_key::EcKey;
use super::pub_key::{build_maced_public_key, validate_public_key};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use ciborium::{cbor, value::Value};
use core::result;
use coset::{iana, AsCborValue, CoseSign1, CoseSign1Builder, HeaderBuilder};
use diced_open_dice::{kdf, DiceArtifacts};
use log::error;
use service_vm_comm::{EcdsaP256KeyPair, GenerateCertificateRequestParams, RequestProcessingError};
use zeroize::Zeroizing;

type Result<T> = result::Result<T, RequestProcessingError>;

const HMAC_SALT: [u8; 32] = [
    0x82, 0x80, 0xFA, 0xD3, 0xA8, 0x0A, 0x9A, 0x4B, 0xF7, 0xA5, 0x7D, 0x7B, 0xE9, 0xC3, 0xAB, 0x13,
    0x89, 0xDC, 0x7B, 0x46, 0xEE, 0x71, 0x22, 0xB4, 0x5F, 0x4C, 0x3F, 0xE2, 0x40, 0x04, 0x3B, 0x6C,
];
const HMAC_KEY_LENGTH: usize = 32;
type HmacKey = Zeroizing<[u8; HMAC_KEY_LENGTH]>;

pub(super) fn generate_ecdsa_p256_key_pair(
    dice_artifacts: &dyn DiceArtifacts,
) -> Result<EcdsaP256KeyPair> {
    let hmac_key = compute_hmac_key(dice_artifacts)?;
    let ec_key = EcKey::new_p256()?;
    let maced_public_key = build_maced_public_key(ec_key.cose_public_key()?, hmac_key.as_ref())?;

    // TODO(b/279425980): Encrypt the private key in a key blob.
    // Remove the printing of the private key.
    log::debug!("Private key: {:?}", ec_key.private_key()?.as_slice());

    let key_pair = EcdsaP256KeyPair { maced_public_key, key_blob: Vec::new() };
    Ok(key_pair)
}

const CSR_PAYLOAD_SCHEMA_V3: u8 = 3;
const AUTH_REQ_SCHEMA_V1: u8 = 1;
// TODO(b/300624493): Add a new certificate type for AVF CSR.
const CERTIFICATE_TYPE: &str = "keymint";

/// Builds the CSR described in:
///
/// hardware/interfaces/security/rkp/aidl/android/hardware/security/keymint/
/// generateCertificateRequestV2.cddl
pub(super) fn generate_certificate_request(
    params: GenerateCertificateRequestParams,
    dice_artifacts: &dyn DiceArtifacts,
) -> Result<Vec<u8>> {
    let hmac_key = compute_hmac_key(dice_artifacts)?;
    let mut public_keys: Vec<Value> = Vec::new();
    for key_to_sign in params.keys_to_sign {
        let public_key = validate_public_key(&key_to_sign, hmac_key.as_ref())?;
        public_keys.push(public_key.to_cbor_value()?);
    }
    // Builds `CsrPayload`.
    let csr_payload = cbor!([
        Value::Integer(CSR_PAYLOAD_SCHEMA_V3.into()),
        Value::Text(String::from(CERTIFICATE_TYPE)),
        // TODO(b/299256925): Add device info in CBOR format here.
        Value::Array(public_keys),
    ])?;
    let csr_payload = cbor_to_vec(&csr_payload)?;

    // Builds `SignedData`.
    let signed_data_payload =
        cbor!([Value::Bytes(params.challenge.to_vec()), Value::Bytes(csr_payload)])?;
    let signed_data = build_signed_data(&signed_data_payload)?.to_cbor_value()?;

    // Builds `AuthenticatedRequest<CsrPayload>`.
    // TODO(b/287233786): Add UdsCerts and DiceCertChain here.
    let uds_certs = Value::Map(Vec::new());
    let dice_cert_chain = Value::Array(Vec::new());
    let auth_req = cbor!([
        Value::Integer(AUTH_REQ_SCHEMA_V1.into()),
        uds_certs,
        dice_cert_chain,
        signed_data,
    ])?;
    cbor_to_vec(&auth_req)
}

fn compute_hmac_key(dice_artifacts: &dyn DiceArtifacts) -> Result<HmacKey> {
    let mut key = Zeroizing::new([0u8; HMAC_KEY_LENGTH]);
    let ikm = &[];
    kdf(ikm, &HMAC_SALT, dice_artifacts.cdi_seal(), key.as_mut()).map_err(|e| {
        error!("Failed to compute the HMAC key: {e}");
        RequestProcessingError::DiceError
    })?;
    Ok(key)
}

/// Builds the `SignedData` for the given payload.
fn build_signed_data(payload: &Value) -> Result<CoseSign1> {
    // TODO(b/299256925): Adjust the signing algorithm if needed.
    let signing_algorithm = iana::Algorithm::ES256;
    let protected = HeaderBuilder::new().algorithm(signing_algorithm).build();
    let signed_data = CoseSign1Builder::new()
        .protected(protected)
        .payload(cbor_to_vec(payload)?)
        .try_create_signature(&[], sign_data)?
        .build();
    Ok(signed_data)
}

fn sign_data(_data: &[u8]) -> Result<Vec<u8>> {
    // TODO(b/287233786): Sign the data with the CDI leaf private key.
    Ok(Vec::new())
}

fn cbor_to_vec(v: &Value) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    ciborium::into_writer(v, &mut data).map_err(coset::CoseError::from)?;
    Ok(data)
}
