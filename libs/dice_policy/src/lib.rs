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

//! A “DICE policy” is a format for setting constraints on a DICE chain. A DICE chain policy
//! verifier takes a policy and a DICE chain, and returns a boolean indicating whether the
//! DICE chain meets the constraints set out on a policy.
//!
//! This forms the foundation of Dice Policy aware Authentication (DPA-Auth), where the server
//! authenticates a client by comparing its dice chain against a set policy.
//!
//! Another use is "sealing", where clients can use an appropriately constructed dice policy to
//! seal a secret. Unsealing is only permitted if dice chain of the component requesting unsealing
//! complies with the policy.
//!
//! A typical policy will assert things like:
//! # DK_pub must have this value
//! # The DICE chain must be exactly five certificates long
//! # authorityHash in the third certificate must have this value
//! securityVersion in the fourth certificate must be an integer greater than 8
//!
//! These constraints used to express policy are (for now) limited to following 2 types:
//! 1. Exact Match: useful for enforcing rules like authority hash should be exactly equal.
//! 2. Greater than or equal to: Useful for setting policies that seal
//! Anti-rollback protected entities (should be accessible to versions >= present).

use anyhow::{anyhow, bail, Context, Result};
use ciborium::cbor;
use ciborium::value::Integer;
use ciborium::Value;
use coset::{AsCborValue, CoseSign1};

const DICE_POLICY_VERSION: u64 = 1;
const AUTHORITY_HASH: i64 = -4670549;
const CONFIG_DESC: i64 = -4670548;
const COMPONENT_NAME: i64 = -70002;

const CONSTRAINT_IDENTIFIER_EXACT_MATCH: &str = "exactMatchConstraint";
const CONSTRAINT_IDENTIFIER_GREATER_OR_EQUAL: &str = "greaterOrEqualToConstraint";

#[derive(Debug)]
struct NodeConstraints(Vec<Constraint>);

#[non_exhaustive]
#[derive(Clone, Debug)]
enum Constraint {
    ExactMatchConstraint { key: KeySpec, value: Value },
    GreaterOrEqualToConstraint { key: KeySpec, value: Integer },
}

#[derive(Clone, Debug)]
struct KeySpec(Vec<Value>);

/// Module for working with dice policy.
#[derive(Debug)]
pub struct DicePolicy {
    version: u64,
    cert_constraints_list: Vec<NodeConstraints>, // Constraint on each entry in dice chain.
}

impl DicePolicy {
    /// Construct a dice policy for anti-rollback protection from a dice chain.
    /// This can be used by clients to construct a policy to seal secrets. The fn uses
    /// hardcoded list of keys needed for building dice policy.
    pub fn for_ar_protected_sealing(dice_chain: &[u8]) -> Result<DicePolicy> {
        // TODO(b/296830692) Add `greaterOrEqualToConstraint` constraint on security_version.
        let keys_to_copy = cbor!([
            ["exactMatchConstraint", AUTHORITY_HASH],
            ["exactMatchConstraint", CONFIG_DESC, COMPONENT_NAME],
        ])?;

        let dice_chain = value_from_bytes(dice_chain).context("Unable to decode top-level CBOR")?;
        let dice_chain = match dice_chain {
            Value::Array(array) if array.len() >= 2 => array,
            _ => bail!("Expected an array of at least length 2, found: {:?}", dice_chain),
        };
        let mut cert_constraints_list: Vec<NodeConstraints> = Vec::new();
        let mut it = dice_chain.into_iter();

        // The first NodeConstraint has ExactMatchConstraint on the full dice cert.
        // We use empty Keyspec for that.
        cert_constraints_list.push(NodeConstraints(vec![Constraint::ExactMatchConstraint {
            key: KeySpec(Vec::new()),
            value: it.next().unwrap(),
        }]));

        for (n, value) in it.enumerate() {
            let entry = cbor_value_from_cose_sign(value)
                .with_context(|| format!("Unable to get Cose payload at: {}", n))?;
            cert_constraints_list.push(payload_to_constraints(entry, keys_to_copy.clone())?);
        }

        Ok(DicePolicy { version: DICE_POLICY_VERSION, cert_constraints_list })
    }

    // TODO: Write CDDL for dice policy.
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
                    Constraint::GreaterOrEqualToConstraint { key, value } => {
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
fn payload_to_constraints(payload: Value, keys_to_copy: Value) -> Result<NodeConstraints> {
    let mut cert_constraints = NodeConstraints(Vec::new());
    let keys_to_copy = keys_to_copy
        .into_array()
        .map_err(|e| anyhow!("Expected array of keys for setting policy, {:?}", e))?;

    for key in keys_to_copy {
        let key = key.into_array().map_err(|e| anyhow!("Expected key to be an array, {:?}", e))?;
        if key.len() < 2 {
            bail!("Expected at least 2 values in key_constraint, found {}", key.len());
        }

        let constraint =
            &*key[0].clone().into_text().map_err(|e| anyhow!("Expected text, {:?}", e))?;

        let val = lookup_value_in_nested_map(&payload, &key[1..])?;
        let constraint = match constraint {
            CONSTRAINT_IDENTIFIER_EXACT_MATCH => {
                Constraint::ExactMatchConstraint { key: KeySpec(key), value: val.clone() }
            }
            CONSTRAINT_IDENTIFIER_GREATER_OR_EQUAL => Constraint::GreaterOrEqualToConstraint {
                key: KeySpec(key),
                value: val.into_integer().map_err(|e| anyhow!("Expected integer, {:?}", e))?,
            },
            _ => bail!("Unknown constraint type"),
        };
        cert_constraints.0.push(constraint);
    }
    Ok(cert_constraints)
}

fn lookup_value_in_nested_map(map: &Value, key_path: &[Value]) -> Result<Value> {
    if key_path.is_empty() {
        return Ok(map.clone());
    }
    // Shadowing the original map which is immutable, this allows
    // easy iteration into the nested map structure.
    let mut map = map.clone();
    for nested_key in key_path.iter() {
        let explicit_map = get_map_from_value(&map)?;
        map = lookup_value_in_map(explicit_map, nested_key)
            .ok_or(anyhow!("Could not find value in map for nested key, {:?}", nested_key))?;
    }
    Ok(map)
}

fn get_map_from_value(val: &Value) -> Result<Vec<(Value, Value)>> {
    match val {
        Value::Bytes(b) => {
            Ok(value_from_bytes(b)?.into_map().map_err(|_| anyhow!("Expected a cbor map "))?)
        }
        Value::Map(map) => Ok(map.clone()),
        _ => bail!("Expected a cbor map"),
    }
}

fn lookup_value_in_map(map: Vec<(Value, Value)>, key: &Value) -> Option<Value> {
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
fn value_from_bytes(mut bytes: &[u8]) -> Result<Value> {
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
    // To analyze a bcc use hwtrust tool from /tools/security/remote_provisioning/hwtrust
    // `hwtrust --verbose dice-chain [path]/composbcc`
    const COMPOS_DICE_CHAIN_SIZE: usize = 5;

    test!(policy_dice_size_is_same);
    fn policy_dice_size_is_same() {
        let input_dice = include_bytes!("../testdata/composbcc");
        let policy = DicePolicy::for_ar_protected_sealing(input_dice).unwrap();
        let _ = policy.serialize_into_cbor().unwrap();
        assert_eq!(policy.cert_constraints_list.len(), COMPOS_DICE_CHAIN_SIZE);
    }
}
