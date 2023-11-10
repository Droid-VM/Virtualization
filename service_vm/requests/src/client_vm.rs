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
//! client VM.

use crate::keyblob::decrypt_private_key;
use alloc::vec::Vec;
use bssl_avf::EcKey;
use ciborium::Value;
use core::result;
use coset::{
    iana::{self, EnumI64},
    CborSerializable, CoseKey, CoseSign, Label,
};
use diced_open_dice::DiceArtifacts;
use log::error;
use service_vm_comm::{ClientVmAttestationParams, Csr, CsrPayload, RequestProcessingError};

type Result<T> = result::Result<T, RequestProcessingError>;

pub(super) fn request_attestation(
    params: ClientVmAttestationParams,
    dice_artifacts: &dyn DiceArtifacts,
) -> Result<Vec<u8>> {
    let csr = Csr::from_cbor_slice(&params.csr)?;
    let cose_sign = CoseSign::from_slice(&csr.signed_csr_payload)?;
    let Some(csr_payload) = cose_sign.payload.as_ref() else {
        error!("No CsrPayload found in the CSR");
        return Err(RequestProcessingError::InternalError);
    };
    let csr_payload = CsrPayload::from_cbor_slice(csr_payload)?;

    // AAD is empty as defined in service_vm/comm/csr/client_vm_csr.CDDL.
    let aad = &[];

    // TODO(b/309440321): Verify the first signature with CDI_Leaf_Pub of
    // the DICE chain in CSR

    let cose_public_key = CoseKey::from_slice(&csr_payload.public_key).unwrap();
    let ec_public_key = to_ec_public_key(&cose_public_key)?;
    cose_sign.verify_signature(1, aad, |signature, message| {
        ec_public_key.ecdsa_p256_verify(signature, message)
    })?;

    // TODO(b/278717513): Compare client VM's DICE chain up to pvmfw cert with
    // RKP VM's DICE chain.

    let _private_key =
        decrypt_private_key(&params.remotely_provisioned_key_blob, dice_artifacts.cdi_seal())
            .map_err(|e| {
                error!("Failed to decrypt the remotely provisioned key blob: {e}");
                RequestProcessingError::FailedToDecryptKeyBlob
            })?;

    // TODO(b/309441500): Build a new certificate signed with the remotely provisioned
    // private key.
    Err(RequestProcessingError::OperationUnimplemented)
}

fn to_ec_public_key(cose_key: &CoseKey) -> Result<EcKey> {
    let x = get_label_value_as_bytes(cose_key, Label::Int(iana::Ec2KeyParameter::X.to_i64()))?;
    let y = get_label_value_as_bytes(cose_key, Label::Int(iana::Ec2KeyParameter::Y.to_i64()))?;
    Ok(EcKey::new_p256_public_key_from_affine_coordinates(x, y)?)
}

fn get_label_value_as_bytes(key: &CoseKey, label: Label) -> Result<&[u8]> {
    Ok(get_label_value(key, label)?.as_bytes().ok_or_else(|| {
        error!("Value not a bstr.");
        RequestProcessingError::PublicKeyDecodingFailed
    })?)
}

fn get_label_value(key: &CoseKey, label: Label) -> Result<&Value> {
    Ok(&key
        .params
        .iter()
        .find(|(k, _)| k == &label)
        .ok_or_else(|| {
            error!("Label {:?} not found in the public key: {:?}", label, key);
            RequestProcessingError::PublicKeyDecodingFailed
        })?
        .1)
}
