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

//! Code to inspect/manipulate the BCC (Dice Chain) we receive from our loader (the hypervisor).

// TODO(b/279910232): Unify this, somehow, with the similar but different code in hwtrust.

use alloc::vec;
use alloc::vec::Vec;
use ciborium::value::Value;
use core::fmt;
use diced_open_dice::{bcc_handover_parse, Cdi, DiceArtifacts, DiceError};
use log::{info, trace};

type Result<T> = core::result::Result<T, BccError>;

pub enum BccError {
    CborDecodeError(ciborium::de::Error<ciborium_io::EndOfFile>),
    CborEncodeError(ciborium::ser::Error<core::convert::Infallible>),
    ExtraneousBytes,
    InvalidBccHandover(DiceError),
    MalformedBcc(&'static str),
    MissingBcc,
}

impl fmt::Debug for BccError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CborDecodeError(e) => write!(f, "Error parsing BCC CBOR: {e:?}"),
            Self::CborEncodeError(e) => write!(f, "Error encoding BCC CBOR: {e:?}"),
            Self::ExtraneousBytes => write!(f, "Unexpected trailing data in BCC"),
            Self::InvalidBccHandover(e) => write!(f, "Invalid BccHandover: {e:?}"),
            Self::MalformedBcc(s) => {
                write!(f, "BCC does not have the expected CBOR structure: {s}")
            }
            Self::MissingBcc => write!(f, "Missing BCC"),
        }
    }
}

pub struct Bcc {
    bcc_entries: Vec<BccEntry>,
    is_debug_mode: bool,
    cdi_seal: Cdi,
    cdi_attest: Cdi,
}

#[repr(transparent)]
struct BccEntry(Value);

#[repr(transparent)]
struct BccPayload(Value);

impl Bcc {
    /// Parse the received BccHandover from CBOR and return a Bcc object containing information
    /// extracted from the handover.
    pub fn new(received_bcc_handover: &[u8]) -> Result<Self> {
        trace!("Original BCC handover: {received_bcc_handover:02x?}");

        let bcc_handover =
            bcc_handover_parse(received_bcc_handover).map_err(BccError::InvalidBccHandover)?;

        trace!("BCC: {bcc_handover:x?}");

        let cdi_seal = *bcc_handover.cdi_seal();
        let cdi_attest = *bcc_handover.cdi_attest();

        let bcc_bytes = bcc_handover.bcc().ok_or(BccError::MissingBcc)?;

        // We don't attempt to fully validate the BCC (e.g. we don't check the signatures) - we
        // have to trust our loader. But if it's invalid CBOR or otherwise clearly ill-formed,
        // something is very wrong, so we fail.
        let bcc_cbor = value_from_bytes(bcc_bytes)?;

        // Bcc = [
        //   PubKeyEd25519 / PubKeyECDSA256, // DK_pub
        //   + BccEntry,                     // Root -> leaf (KM_pub)
        // ]
        let bcc = match bcc_cbor {
            Value::Array(v) if v.len() >= 2 => v,
            _ => return Err(BccError::MalformedBcc("Invalid top level value")),
        };
        let bcc_entries: Vec<_> = bcc.into_iter().skip(1).map(BccEntry::new).collect();
        let is_debug_mode = Self::is_any_entry_debug_mode(bcc_entries.as_slice())?;

        Ok(Self { bcc_entries, is_debug_mode, cdi_seal, cdi_attest })
    }

    /// Returns the CDI seal received in the BccHandover
    pub fn cdi_seal(&self) -> &[u8] {
        &self.cdi_seal
    }

    /// Returns whether any node in the received Dice chain is marked as debug (and hence is not
    /// secure).
    pub fn is_debug_mode(&self) -> bool {
        self.is_debug_mode
    }

    /// "Sanitise" the received BCC by generating a new one which only has the last entry in it.
    /// Returns None if no sanitisation is needed, or otherwise the new encoded BccHandover from
    /// which further Dice derivations can be done.
    pub fn sanitise(&self) -> Result<Option<Vec<u8>>> {
        let count = self.bcc_entries.len();
        if count < 2 {
            info!("BCC sanitisation not needed");
            return Ok(None);
        }

        // Construct a BCC containing only the last entry of the original BCC. We can extract its
        // public key from the previous entry.
        info!("Sanitising BCC");

        // Bcc = [
        //     PubKeyEd25519 / PubKeyECDSA256, // DK_pub
        //     + BccEntry,                     // Root -> leaf (KM_pub)
        // ]
        let last_entry = &self.bcc_entries[count - 1];
        let previous_entry = &self.bcc_entries[count - 2];

        let public_key = previous_entry.payload()?.subject_public_key()?;
        let bcc = vec![public_key, last_entry.0.clone()].into();

        // BccHandover = {
        //   1 : bstr .size 32,     ; CDI_Attest
        //   2 : bstr .size 32,     ; CDI_Seal
        //   ? 3 : Bcc,             ; Certificate chain
        // }
        let bcc_handover: Vec<(Value, Value)> = vec![
            (1.into(), self.cdi_attest.as_slice().into()),
            (2.into(), self.cdi_seal.as_slice().into()),
            (3.into(), bcc),
        ];
        let bcc_handover = value_to_bytes(&bcc_handover.into())?;

        trace!("Sanitised BCC handover: {bcc_handover:02x?}");

        Ok(Some(bcc_handover))
    }

    fn is_any_entry_debug_mode(entries: &[BccEntry]) -> Result<bool> {
        // Check if any entry in the chain is marked as Debug mode, which means the device is not
        // secure. (Normal means it is a secure boot, for that stage at least; we ignore recovery
        // & not configured /invalid values, since it's not clear what they would mean in this
        // context.)
        for entry in entries {
            if entry.payload()?.is_debug_mode()? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl BccEntry {
    pub fn new(entry: Value) -> Self {
        Self(entry)
    }

    pub fn payload(&self) -> Result<BccPayload> {
        // BccEntry = [                                  // COSE_Sign1 (untagged)
        //     protected : bstr .cbor {
        //         1 : AlgorithmEdDSA / AlgorithmES256,  // Algorithm
        //     },
        //     unprotected: {},
        //     payload: bstr .cbor BccPayload,
        //     signature: bstr // PureEd25519(SigningKey, bstr .cbor BccEntryInput) /
        //                     // ECDSA(SigningKey, bstr .cbor BccEntryInput)
        //     // See RFC 8032 for details of how to encode the signature value for Ed25519.
        // ]
        let payload =
            self.payload_bytes().ok_or(BccError::MalformedBcc("Invalid payload in BccEntry"))?;
        let payload = value_from_bytes(payload)?;
        trace!("Bcc payload: {payload:?}");
        Ok(BccPayload(payload))
    }

    fn payload_bytes(&self) -> Option<&Vec<u8>> {
        let entry = self.0.as_array()?;
        if entry.len() != 4 {
            return None;
        };
        entry[2].as_bytes()
    }
}

const KEY_MODE: i32 = -4670551;
const KEY_SUBJECT_PUBLIC_KEY: i32 = -4670552;
const MODE_DEBUG: u8 = 2;

impl BccPayload {
    pub fn is_debug_mode(&self) -> Result<bool> {
        // BccPayload = {                     // CWT
        // ...
        //     ? -4670551 : bstr,             // Mode
        // ...
        // }

        let Some(value) = self.value_from_key(KEY_MODE) else { return Ok(false) };

        // MODE is supposed to be encoded as a 1-byte bstr, but some implementations instead
        // encode it as an integer. Accept either. See b/273552826.
        let mode = if let Some(bytes) = value.as_bytes() {
            if bytes.len() != 1 {
                return Err(BccError::MalformedBcc("Invalid mode bstr"));
            }
            bytes[0].into()
        } else {
            value.as_integer().ok_or(BccError::MalformedBcc("Invalid type for mode"))?
        };
        Ok(mode == MODE_DEBUG.into())
    }

    pub fn subject_public_key(&self) -> Result<Value> {
        // BccPayload = {                     // CWT
        // ...
        //     -4670552 : bstr .cbor PubKeyEd25519 /
        //                bstr .cbor PubKeyECDSA256   // Subject Public Key
        // ...
        // }
        let public_key = self
            .value_from_key(KEY_SUBJECT_PUBLIC_KEY)
            .ok_or(BccError::MalformedBcc("Payload missing subject public key"))?;
        // The BccPayload stores the key as encoded bytes, but we need it as a CBOR value.
        let bytes =
            public_key.as_bytes().ok_or(BccError::MalformedBcc("Invalid subject public key"))?;
        value_from_bytes(bytes)
    }

    fn value_from_key(&self, key: i32) -> Option<&Value> {
        // BccPayload is just a map; we only use integral keys, but in general it's legitimate
        // for other things to be present, or for the key we care about not to be present.
        // Ciborium represents the map as a Vec, preserving order (and allowing duplicate keys,
        // which we ignore) but preventing fast lookup.
        let payload = self.0.as_map()?;
        for (k, v) in payload {
            let Some(k) = k.as_integer() else { continue };
            if k == key.into() {
                return Some(v);
            }
        }
        None
    }
}

/// Decodes the provided binary CBOR-encoded value and returns a
/// ciborium::Value struct wrapped in Result.
fn value_from_bytes(mut bytes: &[u8]) -> Result<Value> {
    let value = ciborium::de::from_reader(&mut bytes).map_err(BccError::CborDecodeError)?;
    // Ciborium tries to read one Value, but doesn't care if there is trailing data after it. We do.
    if !bytes.is_empty() {
        return Err(BccError::ExtraneousBytes);
    }
    Ok(value)
}

/// Encodes a ciborium::Value into bytes.
fn value_to_bytes(value: &Value) -> Result<Vec<u8>> {
    let mut bytes: Vec<u8> = Vec::new();
    ciborium::ser::into_writer(&value, &mut bytes).map_err(BccError::CborEncodeError)?;
    Ok(bytes)
}
