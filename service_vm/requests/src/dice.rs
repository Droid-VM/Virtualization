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
#![allow(dead_code)]
use alloc::vec::Vec;
use ciborium::value::Value;
use core::result;
use coset::{self, AsCborValue, CborSerializable, CoseError, CoseKey, CoseSign1};
use diced_open_dice::DiceMode;
use log::error;
use service_vm_comm::{cbor_value_type, try_as_bytes, RequestProcessingError};

type Result<T> = result::Result<T, RequestProcessingError>;

const ISS: i64 = 1;
const SUB: i64 = 2;
const CODE_HASH: i64 = -4670545;
const CODE_DESC: i64 = -4670546;
const CONFIG_HASH: i64 = -4670547;
const CONFIG_DESC: i64 = -4670548;
const AUTHORITY_HASH: i64 = -4670549;
const AUTHORITY_DESC: i64 = -4670550;
const MODE: i64 = -4670551;
const SUBJECT_PUBLIC_KEY: i64 = -4670552;
const KEY_USAGE: i64 = -4670553;
const PROFILE_NAME: i64 = -4670554;

const CONFIG_DESC_RESERVED_MAX: i64 = -70000;
const CONFIG_DESC_RESERVED_MIN: i64 = -70999;
const COMPONENT_NAME: i64 = -70002;
const COMPONENT_VERSION: i64 = -70003;
const RESETTABLE: i64 = -70004;
const SECURITY_VERSION: i64 = -70005;
const RKP_VM_MARKER: i64 = -70006;

/// Represents a `DiceCertChain` defined as following:
///
/// DiceCertChain = [
///     PubKeyEd25519 / PubKeyECDSA256 / PubKeyECDSA384,  ; UDS_Pub
///     + DiceChainEntry,               ; First CDI_Certificate -> Last CDI_Certificate
/// ]
pub(crate) struct Chain {
    pub(crate) root_public_key: PublicKey,
    pub(crate) payloads: Vec<Payload>,
}

impl Chain {
    /// Verifies and creates a DICE chain from the provided CBOR-encoded slice.
    pub(crate) fn verify_cbor_slice(data: &[u8]) -> Result<Self> {
        let value = Value::from_slice(data)?;
        let Value::Array(mut arr) = value else {
            return Err(CoseError::UnexpectedItem(cbor_value_type(&value), "array").into());
        };
        if arr.len() <= 1 {
            error!("Malformed DICE chain with less than two entries");
            return Err(RequestProcessingError::InvalidDiceChain);
        }
        let root_public_key = CoseKey::from_cbor_value(arr.remove(0))?.into();

        let mut payloads = Vec::with_capacity(arr.len());
        let mut previous_public_key = &root_public_key;
        for (i, value) in arr.into_iter().enumerate() {
            let payload = Payload::verify_cbor_value(value, previous_public_key).map_err(|e| {
                error!("Failed to verify the DICE chain entry {}: {:?}", i, e);
                e
            })?;
            payloads.push(payload);
            previous_public_key = payloads.last().unwrap().subject_public_key();
        }
        Ok(Self { root_public_key, payloads })
    }

    /// Gets the last payload in the chain.
    pub fn leaf(&self) -> &Payload {
        // There is always at least one payload as checked in `from_cbor_slice`.
        self.payloads.last().unwrap()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PublicKey(CoseKey);

impl From<CoseKey> for PublicKey {
    fn from(key: CoseKey) -> Self {
        Self(key)
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
}

impl Payload {
    fn verify_cbor_value(value: Value, _authority_public_key: &PublicKey) -> Result<Self> {
        let cose_sign1 = CoseSign1::from_cbor_value(value)?;
        // TODO(b/310931749): Verify the signature with `authority_public_key`.
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
    subject_public_key: Option<PublicKey>,
    mode: Option<DiceMode>,
}

impl PayloadBuilder {
    fn subject_public_key(&mut self, key: PublicKey) -> Result<()> {
        if self.subject_public_key.is_some() {
            error!("Subject public key is duplicated in the Payload");
            return Err(RequestProcessingError::InvalidDiceChain);
        }
        self.subject_public_key = Some(key);
        Ok(())
    }

    fn mode(&mut self, mode: DiceMode) -> Result<()> {
        if self.mode.is_some() {
            error!("Mode is duplicated in the Payload");
            return Err(RequestProcessingError::InvalidDiceChain);
        }
        self.mode = Some(mode);
        Ok(())
    }

    fn build(self) -> Result<Payload> {
        let subject_public_key = self.subject_public_key.ok_or_else(|| {
            error!("Subject public key is missing in the Payload");
            RequestProcessingError::InvalidDiceChain
        })?;
        let mode = self.mode.ok_or_else(|| {
            error!("Mode is missing in the Payload");
            RequestProcessingError::InvalidDiceChain
        })?;
        Ok(Payload { subject_public_key, mode })
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
                builder.subject_public_key(CoseKey::from_slice(&subject_public_key)?.into())?;
            }
            MODE => {
                // TODO(b/313428920): Parse the correct DICE mode.
                builder.mode(DiceMode::kDiceModeDebug)?;
            }
            ISS | SUB | CODE_HASH | CODE_DESC | AUTHORITY_HASH | AUTHORITY_DESC | CONFIG_HASH
            | CONFIG_DESC | KEY_USAGE | PROFILE_NAME => {}
            _ => {
                error!("Invalid key found in the DICE chain entry: {:?}", key);
                return Err(RequestProcessingError::InvalidDiceChain);
            }
        }
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use diced_open_dice::DiceArtifacts;

    #[test]
    fn parsing_valid_dice_chain_succeeds() -> Result<()> {
        let dice_artifacts = diced_sample_inputs::make_sample_bcc_and_cdis().unwrap();
        let chain = Chain::verify_cbor_slice(dice_artifacts.bcc().unwrap())?;
        let cdi_leaf_pub = chain.leaf().subject_public_key();
        assert!(!cdi_leaf_pub.0.params.is_empty());
        Ok(())
    }
}
