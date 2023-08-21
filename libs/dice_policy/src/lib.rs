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

// A “DICE policy” is a format for setting constraints on a DICE chain.
// A DICE chain policy verifier takes a policy and a DICE chain, and returns a boolean indicating
// whether the DICE chain meets the constraints set out on a policy.

//! Library to create dice policy from a dice chain. The keys for setting the constraints are hard-coded.
use anyhow::{anyhow, bail, ensure, Context, Result};
use ciborium::cbor;
use ciborium::value::Integer;
use ciborium::Value;
use coset::{AsCborValue, CoseKey, CoseSign1};

const DICE_POLICY_VERSION: u64 = 1;
const AUTHORITY_HASH: i64 = -4670549;
const CONFIG_DESC: i64 = -4670548;
const COMPONENT_NAME: i64 = -70002;

const CONSTRAINT_IDENTIFIER_EXACT_MATCH: &str = "exactMatchConstraint";
const CONSTRAINT_IDENTIFIER_GE_EQUAL: &str = "geConstraint";

#[derive(Debug)]
struct CertConstraints(Vec<Constraint>);

#[derive(Clone, Debug)]
enum Constraint {
    ExactMatchConstraint { key: KeySpec, value: Value },
    GeConstraint { key: KeySpec, value: Integer },
}

#[derive(Clone, Debug)]
struct KeySpec(Vec<Value>);

/// Dice policy is a set of constraints on a Dice chain. These constraints are limited to following 2 types:
/// 1. Exact Match: useful for enforcing rules like authority hash should be exactly equal.
/// 2. Greater than equal to: Useful for setting policies that seal
///    Anti-rollback protected entities (should be accessible to versions >= present).
#[derive(Debug)]
pub struct DicePolicy {
    version: u64,
    cert_constraints_list: Vec<CertConstraints>, // Constraint on each entry in dice chain.
}

impl DicePolicy {
    /// Construct a dice policy from the dice chain. This can be used by clients to construct a policy to seal secrets.
    /// This function uses hardcoded list of keys needed for building constraints.
    pub fn from_dice_chain(dice_chain: &[u8]) -> Result<DicePolicy> {
        // TODO(b/296830692) Add `geConstraint` constraint on security_version as well.
        let keys_to_copy = cbor!([
            ["exactMatchConstraint", AUTHORITY_HASH],
            ["exactMatchConstraint", CONFIG_DESC, COMPONENT_NAME],
        ])?;

        let dice_chain = value_from_bytes(dice_chain).context("Unable to decode top-level CBOR")?;
        let dice_chain = match dice_chain {
            Value::Array(array) if array.len() >= 2 => array,
            _ => bail!("Expected an array of at least length 2, found: {:?}", dice_chain),
        };
        let mut cert_constraints_list: Vec<CertConstraints> = Vec::new();
        let mut it = dice_chain.into_iter();

        let root_public_key = CoseKey::from_cbor_value(it.next().unwrap())
            .map_err(|e| anyhow!("Error parsing root public key CBOR: {}", e))?;
        cert_constraints_list.push(CertConstraints(vec![Constraint::ExactMatchConstraint {
            key: KeySpec(Vec::new()),
            value: root_public_key
                .to_cbor_value()
                .map_err(|_| anyhow!("Unable to convert root public key to cbor Value"))?,
        }]));

        for (n, value) in it.enumerate() {
            let entry = cbor_value_from_cose_sign(value)
                .with_context(|| format!("Unable to get Cose payload at: {}", n))?;
            cert_constraints_list.push(payload_to_constraints(entry, keys_to_copy.clone())?);
        }

        Ok(DicePolicy { version: DICE_POLICY_VERSION, cert_constraints_list })
    }

    /// Serialize the Dice policy into a CBOR Value.
    pub fn serialize_into_cbor(&self) -> Result<Value> {
        let mut policy = vec![Value::from(self.version)];
        for cert_constraints in &self.cert_constraints_list {
            let mut cert_constraints_cbor: Vec<(Value, Value)> = vec![];
            for constraint in cert_constraints.0.clone() {
                let constraint_cbor = match constraint {
                    Constraint::ExactMatchConstraint { key, value } => {
                        (Value::from(key.0), value.clone())
                    }
                    Constraint::GeConstraint { key, value } => {
                        (Value::from(key.0), Value::Integer(value))
                    }
                };
                cert_constraints_cbor.push(constraint_cbor);
            }
            policy.push(Value::Map(cert_constraints_cbor));
        }
        Ok(Value::Array(policy))
    }
}

// Take the payload of a dice cert & construct the constraints on it.
fn payload_to_constraints(payload: Value, keys_to_copy: Value) -> Result<CertConstraints> {
    let mut cert_constraints = CertConstraints(Vec::new());
    let keys_to_copy = keys_to_copy
        .into_array()
        .map_err(|e| anyhow!("Expected array of keys for setting policy, {:?}", e))?;

    for key in keys_to_copy {
        let key = key.into_array().map_err(|e| anyhow!("Expected key to be an array, {:?}", e))?;
        ensure!(
            key.len() >= 2,
            "Expected at least 2 values in key_constraint, found {}",
            key.len()
        );
        let mut val = payload.clone();
        for nested_key in key[1..].iter() {
            if val.is_bytes() {
                val = value_from_bytes(&val.into_bytes().unwrap())?;
            }
            let map = val.clone().into_map().map_err(|e| anyhow!("Expected a map, {:?}", e))?;
            val = find_value_in_map(map, nested_key)
                .ok_or(anyhow!("Could not find value in map for nested key, {:?}", nested_key))?;
        }
        let constraint =
            &*key[0].clone().into_text().map_err(|e| anyhow!("Expected Cbor Text, {:?}", e))?;
        let constraint = match constraint {
            CONSTRAINT_IDENTIFIER_EXACT_MATCH => {
                Constraint::ExactMatchConstraint { key: KeySpec(key), value: val.clone() }
            }
            CONSTRAINT_IDENTIFIER_GE_EQUAL => Constraint::GeConstraint {
                key: KeySpec(key),
                value: val
                    .into_integer()
                    .map_err(|e| anyhow!("Unable to convert to cbor integer, {:?}", e))?,
            },
            _ => bail!("Unknown constraint type"),
        };
        cert_constraints.0.push(constraint);
    }
    Ok(cert_constraints)
}

fn find_value_in_map(map: Vec<(Value, Value)>, key: &Value) -> Option<Value> {
    for (k, v) in map.into_iter() {
        if k == *key {
            return Some(v);
        }
    }
    None
}

/// Extract the payload from the COSE Sign
fn cbor_value_from_cose_sign(cbor: Value) -> Result<Value> {
    let sign1 =
        CoseSign1::from_cbor_value(cbor).map_err(|e| anyhow!("Error extracting CoseKey: {}", e))?;
    match sign1.payload {
        None => bail!("Missing payload"),
        Some(payload) => Ok(value_from_bytes(&payload)?),
    }
}

/// Decodes the provided binary CBOR-encoded value and returns a
/// ciborium::Value struct wrapped in Result.
pub fn value_from_bytes(mut bytes: &[u8]) -> Result<Value> {
    let value = ciborium::de::from_reader(&mut bytes)?;
    Ok(value)
}

#[cfg(test)]
rdroidtest::test_main!();

#[cfg(test)]
mod tests {
    use super::*;
    use rdroidtest::test;
    // This is the number of certs in compos bcc (including the first ROT)
    // To analyze a bcc use aosp project /tools/security/remote_provisioning/hwtrust
    // `cargo run -- --verbose verify-dice-chain [path]/composbcc`
    const COMPOS_DICE_CHAIN_SIZE: usize = 5;

    test!(policy_dice_size_is_same);
    fn policy_dice_size_is_same() {
        let input_dice = include_bytes!("../testdata/composbcc");
        let policy = DicePolicy::from_dice_chain(input_dice).unwrap();
        let _ = policy.serialize_into_cbor().unwrap();
        assert_eq!(policy.cert_constraints_list.len(), COMPOS_DICE_CHAIN_SIZE);
    }
}
