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

//! Logic for managing dice chain.

use crate::dice_payload::{value_from_bytes, Payload};
use anyhow::{anyhow, bail, Context, Result};
use ciborium::Value;
use coset::{AsCborValue, CoseKey, CoseSign1};
use log::info;

#[derive(Debug)]
pub struct DiceChain {
    // Just keeping CoseKey, maybe keep it private
    pub root_public_key: CoseKey,
    // This can be &[u8]
    pub payloads: Vec<Payload>,
}

impl DiceChain {
    // From bcc
    pub fn from_cbor(bytes: &[u8]) -> Result<Self> {
        let dice_cbor = value_from_bytes(bytes).context("Unable to decode top-level CBOR")?;
        let array = match dice_cbor {
            Value::Array(array) if array.len() >= 2 => array,
            _ => bail!("Expected an array of at least length 2, found: {:?}", dice_cbor),
        };
        let mut it = array.into_iter();
        // TODO: What are the allowed formats of COSE key & if it impacts the  policy building
        // let root_public_key = cose_key_from_cbor_value(it.next().unwrap())
        //     .context("Error parsing root public key CBOR")?;
        let root_public_key = CoseKey::from_cbor_value(it.next().unwrap())
            .map_err(|e| anyhow!("Error extracting CoseKey: {}", e))?;
        info!("Root public key: {:?}", root_public_key);
        let mut payloads = Vec::with_capacity(it.len());
        for (n, value) in it.enumerate() {
            let entry = cbor_value_from_cose_sign(value)
                .with_context(|| format!("Failed to get value of payload at: {}", n))?;
            let payload = Payload::from_cbor(&entry)
                .with_context(|| format!("Invalid payload at index {}", n))?;
            payloads.push(payload);
        }

        Ok(Self { root_public_key, payloads })
    }
}

fn cbor_value_from_cose_sign(cbor: Value) -> Result<Vec<u8>> {
    let sign1 =
        CoseSign1::from_cbor_value(cbor).map_err(|e| anyhow!("Error extracting CoseKey: {}", e))?;
    match sign1.payload {
        None => bail!("Missing payload"),
        Some(payload) => Ok(payload),
    }
}
