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

//! This module contains functions related to DICE.

use alloc::vec::Vec;
use ciborium::value::Value;
use core::cell::OnceCell;
use core::result;
use coset::{
    self, iana, AsCborValue, CborSerializable, CoseError, CoseKey, CoseSign1, KeyOperation,
};
use diced_open_dice::{DiceMode, HASH_SIZE};
use log::error;
use service_vm_comm::{cbor_value_type, try_as_bytes, RequestProcessingError};

type Result<T> = result::Result<T, RequestProcessingError>;

const CODE_HASH: i64 = -4670545;
const CONFIG_DESC: i64 = -4670548;
const AUTHORITY_HASH: i64 = -4670549;
const MODE: i64 = -4670551;
const SUBJECT_PUBLIC_KEY: i64 = -4670552;

/// Represents a `DiceCertChain` defined as following:
///
/// DiceCertChain = [
///     PubKeyEd25519 / PubKeyECDSA256 / PubKeyECDSA384,  ; UDS_Pub
///     + DiceChainEntry,               ; First CDI_Certificate -> Last CDI_Certificate
/// ]
pub(crate) struct Chain {
    pub(crate) payloads: Vec<Payload>,
}

impl Chain {
    /// Verifies and creates a DICE chain from the provided CBOR-encoded slice.
    ///
    /// The Client VM's DICE chain must match RKP VM's DICE chain up to the pvmfw payload.
    pub(crate) fn verify_cbor_slice(
        client_vm_dice_chain: &[u8],
        service_vm_dice_chain: &[u8],
    ) -> Result<Self> {
        let mut client_vm_dice_chain =
            try_as_value_array(Value::from_slice(client_vm_dice_chain)?, "client_vm_dice_chain")?;
        let service_vm_dice_chain =
            try_as_value_array(Value::from_slice(service_vm_dice_chain)?, "service_vm_dice_chain")?;
        verify_dice_chain_up_to_pvmfw_payload(&client_vm_dice_chain, &service_vm_dice_chain)?;

        let root_public_key =
            CoseKey::from_cbor_value(client_vm_dice_chain.remove(0))?.try_into()?;

        let mut payloads = Vec::with_capacity(client_vm_dice_chain.len());
        let mut previous_public_key = &root_public_key;
        for (i, value) in client_vm_dice_chain.into_iter().enumerate() {
            let payload = Payload::verify_cbor_value(value, previous_public_key).map_err(|e| {
                error!("Failed to verify the DICE chain entry {}: {:?}", i, e);
                e
            })?;
            payloads.push(payload);
            previous_public_key = payloads.last().unwrap().subject_public_key();
        }
        Ok(Self { payloads })
    }

    /// Gets the last payload in the chain.
    pub fn leaf(&self) -> &Payload {
        // There is always at least one payload as checked in `from_cbor_slice`.
        self.payloads.last().unwrap()
    }
}

fn verify_dice_chain_up_to_pvmfw_payload(
    client_vm_dice_chain: &Vec<Value>,
    service_vm_dice_chain: &Vec<Value>,
) -> Result<()> {
    if service_vm_dice_chain.len() < 3 {
        // The service VM's DICE chain must contain at least three entries:
        //   - The root public key
        //   - The pvmfw payload
        //   - The RKP VM payload
        error!("The service VM DICE chain must contain at least three entries");
        return Err(RequestProcessingError::InternalError);
    }
    // Ignores the last payload that describes RKP VM
    let entries_up_to_pvmfw = &service_vm_dice_chain[0..(service_vm_dice_chain.len() - 1)];
    if client_vm_dice_chain.len() == service_vm_dice_chain.len() + 2 {
        // Client VM DICE chain = entries_up_to_pvmfw
        //    + Microdroid kernel payload (added in pvmfw)
        //    + Apk/Apexes payload (added in microdroid)
        error!("The client VM's DICE chain must contain exactly two extra payload entries");
        return Err(RequestProcessingError::InvalidDiceChain);
    }
    if entries_up_to_pvmfw == &client_vm_dice_chain[0..entries_up_to_pvmfw.len()] {
        error!(
            "The client VM's DICE chain does not match RKP VM's DICE chain up to the pvmfw payload"
        );
        return Err(RequestProcessingError::DiceChainUnmatch);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PublicKey(CoseKey);

impl TryFrom<CoseKey> for PublicKey {
    type Error = RequestProcessingError;

    fn try_from(key: CoseKey) -> Result<Self> {
        if !key.key_ops.contains(&KeyOperation::Assigned(iana::KeyOperation::Verify)) {
            error!("Public key does not support verification");
            return Err(RequestProcessingError::InvalidDiceChain);
        }
        Ok(Self(key))
    }
}

impl PublicKey {
    /// Verifies the signature of the provided message with this public key.
    pub(crate) fn verify(&self, _signature: &[u8], _message: &[u8]) -> Result<()> {
        // TODO(b/310931749): Implement the verification function.
        Ok(())
    }
}

/// Represents a `DiceChainEntryPayload` described in:
///
/// hardware/interfaces/security/rkp/aidl/android/hardware/security/keymint/
/// generateCertificateRequestV2.cddl
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Payload {
    subject_public_key: PublicKey,
    mode: DiceMode,
    code_hash: [u8; HASH_SIZE],
    authority_hash: [u8; HASH_SIZE],
    config_descriptor: Vec<u8>,
}

impl Payload {
    fn verify_cbor_value(value: Value, authority_public_key: &PublicKey) -> Result<Self> {
        let cose_sign1 = CoseSign1::from_cbor_value(value)?;
        let aad = &[]; // No AAD is used in the DICE chain.
        cose_sign1.verify_signature(aad, |signature, message| {
            authority_public_key.verify(signature, message)
        })?;

        let payload = cose_sign1.payload.ok_or_else(|| {
            error!("No payload found in the DICE chain entry");
            RequestProcessingError::InvalidDiceChain
        })?;
        let payload = Value::from_slice(&payload)?;
        let Value::Map(entries) = payload else {
            return Err(CoseError::UnexpectedItem(cbor_value_type(&payload), "map").into());
        };
        build_payload(entries)
    }

    /// Gets the subject public key.
    pub(crate) fn subject_public_key(&self) -> &PublicKey {
        &self.subject_public_key
    }
}

#[derive(Default, Debug, Clone)]
struct PayloadBuilder {
    subject_public_key: OnceCell<PublicKey>,
    mode: OnceCell<DiceMode>,
    code_hash: OnceCell<[u8; HASH_SIZE]>,
    authority_hash: OnceCell<[u8; HASH_SIZE]>,
    config_descriptor: OnceCell<Vec<u8>>,
}

fn set_once<T>(field: &OnceCell<T>, value: T, field_name: &str) -> Result<()> {
    field.set(value).map_err(|_| {
        error!("Field '{field_name}' is duplicated in the Payload");
        RequestProcessingError::InvalidDiceChain
    })
}

fn take_value<T>(field: &mut OnceCell<T>, field_name: &str) -> Result<T> {
    field.take().ok_or_else(|| {
        error!("Field '{field_name}' is missing in the Payload");
        RequestProcessingError::InvalidDiceChain
    })
}

impl PayloadBuilder {
    fn subject_public_key(&mut self, key: PublicKey) -> Result<()> {
        set_once(&self.subject_public_key, key, "subject_public_key")
    }

    fn mode(&mut self, mode: DiceMode) -> Result<()> {
        set_once(&self.mode, mode, "mode")
    }

    fn code_hash(&mut self, code_hash: [u8; HASH_SIZE]) -> Result<()> {
        set_once(&self.code_hash, code_hash, "code_hash")
    }

    fn authority_hash(&mut self, authority_hash: [u8; HASH_SIZE]) -> Result<()> {
        set_once(&self.authority_hash, authority_hash, "authority_hash")
    }

    fn config_descriptor(&mut self, config_descriptor: Vec<u8>) -> Result<()> {
        set_once(&self.config_descriptor, config_descriptor, "config_descriptor")
    }

    fn build(mut self) -> Result<Payload> {
        let subject_public_key = take_value(&mut self.subject_public_key, "subject_public_key")?;
        let mode = take_value(&mut self.mode, "mode")?;
        let code_hash = take_value(&mut self.code_hash, "code_hash")?;
        let authority_hash = take_value(&mut self.authority_hash, "authority_hash")?;
        let config_descriptor = take_value(&mut self.config_descriptor, "config_descriptor")?;
        Ok(Payload { subject_public_key, mode, code_hash, authority_hash, config_descriptor })
    }
}

fn build_payload(entries: Vec<(Value, Value)>) -> Result<Payload> {
    let mut builder = PayloadBuilder::default();
    for (key, value) in entries.into_iter() {
        let Some(Ok(key)) = key.as_integer().map(i64::try_from) else {
            error!("Invalid key found in the DICE chain entry: {:?}", key);
            return Err(RequestProcessingError::InvalidDiceChain);
        };
        match key {
            SUBJECT_PUBLIC_KEY => {
                let subject_public_key = try_as_bytes(value, "subject_public_key")?;
                let subject_public_key = CoseKey::from_slice(&subject_public_key)?.try_into()?;
                builder.subject_public_key(subject_public_key)?;
            }
            MODE => {
                // TODO(b/313428920): Parse DiceMode from the CBOR value.
                builder.mode(DiceMode::kDiceModeDebug)?;
            }
            CODE_HASH => builder.code_hash(try_as_byte_array(value, "code_hash")?)?,
            AUTHORITY_HASH => {
                builder.authority_hash(try_as_byte_array(value, "authority_hash")?)?
            }
            CONFIG_DESC => builder.config_descriptor(try_as_bytes(value, "config_descriptor")?)?,
            _ => {}
        }
    }
    builder.build()
}

fn try_as_value_array(v: Value, context: &str) -> coset::Result<Vec<Value>> {
    if let Value::Array(data) = v {
        Ok(data)
    } else {
        let v_type = cbor_value_type(&v);
        error!("The provided value type '{v_type}' is not of type 'bytes': {context}");
        Err(CoseError::UnexpectedItem(v_type, "array"))
    }
}

fn try_as_byte_array<const N: usize>(v: Value, context: &str) -> Result<[u8; N]> {
    let data = try_as_bytes(v, context)?;
    data.try_into().map_err(|e| {
        error!("The provided value '{context}' is not an array of length {N}: {e:?}");
        RequestProcessingError::InternalError
    })
}
