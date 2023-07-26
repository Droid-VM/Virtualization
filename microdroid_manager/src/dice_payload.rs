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

//! Logic for managing each of the dice certificate in chain.

use anyhow::{anyhow, bail, Context, Result};
use ciborium::Value;

const ISS: i64 = 1;
const SUB: i64 = 2;
// const CODE_HASH: i64 = -4670545;
// const CODE_DESC: i64 = -4670546;
// const CONFIG_HASH: i64 = -4670547;
// const CONFIG_DESC: i64 = -4670548;
// const AUTHORITY_HASH: i64 = -4670549;
// const AUTHORITY_DESC: i64 = -4670550;
// const MODE: i64 = -4670551;
const SUBJECT_PUBLIC_KEY: i64 = -4670552;
// const KEY_USAGE: i64 = -4670553;

// const CONFIG_DESC_RESERVED_MAX: i64 = -70000;
// const CONFIG_DESC_RESERVED_MIN: i64 = -70999;
// const COMPONENT_NAME: i64 = -70002;
// const COMPONENT_VERSION: i64 = -70003;
// const RESETTABLE: i64 = -70004;
// const SECURITY_VERSION: i64 = -70005;

/// The payload of a DICE chain entry - this is a subset of fields. These are to be mandated
/// by CDD to enable rollback protected secrets.
#[derive(Debug)]
pub struct Payload {
    issuer: String,
    subject: String,
    subject_public_key: Vec<u8>,
    // mode: DiceMode,
    // code_desc: Option<Vec<u8>>,
    // code_hash: Vec<u8>,
    // config_desc: ConfigDesc,
    // config_hash: Option<Vec<u8>>,
    // authority_desc: Option<Vec<u8>>,
    // authority_hash: Vec<u8>,
}

impl Payload {
    pub fn from_cbor(bytes: &[u8]) -> Result<Self> {
        let mut payload_builder = PayloadBuilder::default();

        let entries = cbor_map_from_slice(bytes)?;
        for (key, value) in entries.into_iter() {
            if let Some(Ok(key)) = key.as_integer().map(TryInto::try_into) {
                match key {
                    // TODO: Do not unwrap - change this to something sensible
                    ISS => payload_builder.issuer(value.into_text().unwrap()),
                    SUB => payload_builder.subject(value.into_text().unwrap()),
                    SUBJECT_PUBLIC_KEY => payload_builder.subject_public_key(
                        value
                            .into_bytes()
                            .map_err(|_| anyhow!("error getting in public key format"))?,
                    ),
                    // MODE => &mut mode,
                    // CODE_DESC => &mut code_desc,
                    // CODE_HASH => &mut code_hash,
                    // CONFIG_DESC => &mut config_desc,
                    // CONFIG_HASH => &mut config_hash,
                    // AUTHORITY_DESC => &mut authority_desc,
                    // AUTHORITY_HASH => &mut authority_hash,
                    // KEY_USAGE => &mut key_usage,
                    _ => bail!("Unknown key {}", key),
                };
            } else {
                bail!("Invalid key: {:?}", key);
            }
        }
        Ok(payload_builder.build()?)
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub(crate) enum PayloadBuilderError {
    #[error("issuer empty")]
    Issuer,
    #[error("subject empty")]
    Subject,
    #[error("subject public key empty")]
    SubjectPublicKeyEmpty,
}

fn cbor_map_from_slice(bytes: &[u8]) -> Result<Vec<(Value, Value)>> {
    let value = value_from_bytes(bytes).context("Error parsing CBOR into a map")?;
    let entries = match value {
        Value::Map(entries) => entries,
        _ => bail!("Not a map: {:?}", value),
    };
    Ok(entries)
}

pub(crate) struct PayloadBuilder(Payload);

impl PayloadBuilder {
    /// Constructs a new builder with the given subject public key.
    pub fn default() -> Self {
        Self(Payload {
            issuer: Default::default(),
            subject: Default::default(),
            subject_public_key: Default::default(),
            // mode: Default::default(),
            // code_desc: Default::default(),
            // code_hash: Default::default(),
            // config_desc: Default::default(),
            // config_hash: Default::default(),
            // authority_desc: Default::default(),
            // authority_hash: Default::default(),
        })
    }

    /// Builds the [`Payload`] after validating the fields.
    pub fn build(self) -> Result<Payload, PayloadBuilderError> {
        if self.0.issuer.is_empty() {
            return Err(PayloadBuilderError::Issuer);
        }
        if self.0.subject.is_empty() {
            return Err(PayloadBuilderError::Subject);
        }
        if self.0.subject_public_key.is_empty() {
            return Err(PayloadBuilderError::SubjectPublicKeyEmpty);
        }
        // let used_hash_size = self.0.code_hash.len();
        // if ![32, 48, 64].contains(&used_hash_size) {
        //     return Err(PayloadBuilderError::CodeHashSize);
        // }
        // if let Some(ref config_hash) = self.0.config_hash {
        //     if config_hash.len() != used_hash_size {
        //         return Err(PayloadBuilderError::ConfigHashSize);
        //     }
        // }
        // if self.0.authority_hash.len() != used_hash_size {
        //     return Err(PayloadBuilderError::AuthorityHashSize);
        // }
        Ok(self.0)
    }

    /// Sets the issuer of the payload.
    #[must_use]
    pub fn issuer<S: Into<String>>(&mut self, issuer: S) -> &mut Self {
        self.0.issuer = issuer.into();
        self
    }

    /// Sets the subject of the payload.
    #[must_use]
    pub fn subject<S: Into<String>>(&mut self, subject: S) -> &mut Self {
        self.0.subject = subject.into();
        self
    }

    /// Sets the code hash of the payload.
    #[must_use]
    pub fn subject_public_key(&mut self, subject_public_key: Vec<u8>) -> &mut Self {
        self.0.subject_public_key = subject_public_key;
        self
    }

    // /// Sets the mode of the payload.
    // #[must_use]
    // pub fn mode(mut self, mode: DiceMode) -> Self {
    //     self.0.mode = mode;
    //     self
    // }

    // /// Sets the code descriptor of the payload.
    // #[must_use]
    // pub fn code_desc(mut self, code_desc: Option<Vec<u8>>) -> Self {
    //     self.0.code_desc = code_desc;
    //     self
    // }

    // /// Sets the code hash of the payload.
    // #[must_use]
    // pub fn code_hash(mut self, code_hash: Vec<u8>) -> Self {
    //     self.0.code_hash = code_hash;
    //     self
    // }

    // /// Sets the configuration descriptor of the payload.
    // #[must_use]
    // pub fn config_desc(mut self, config_desc: ConfigDesc) -> Self {
    //     self.0.config_desc = config_desc;
    //     self
    // }

    // /// Sets the configuration hash of the payload.
    // #[must_use]
    // pub fn config_hash(mut self, config_hash: Option<Vec<u8>>) -> Self {
    //     self.0.config_hash = config_hash;
    //     self
    // }

    // /// Sets the authority descriptor of the payload.
    // #[must_use]
    // pub fn authority_desc(mut self, authority_desc: Option<Vec<u8>>) -> Self {
    //     self.0.authority_desc = authority_desc;
    //     self
    // }

    // /// Sets the authority hash of the payload.
    // #[must_use]
    // pub fn authority_hash(mut self, authority_hash: Vec<u8>) -> Self {
    //     self.0.authority_hash = authority_hash;
    //     self
    // }
}

/// Decodes the provided binary CBOR-encoded value and returns a
/// ciborium::Value struct wrapped in Result.
pub fn value_from_bytes(mut bytes: &[u8]) -> Result<Value> {
    let value = ciborium::de::from_reader(&mut bytes)?;
    // Ciborium tries to read one Value, but doesn't care if there is trailing data after it. We do.
    if !bytes.is_empty() {
        bail!("Extra bytes");
    }
    Ok(value)
}
