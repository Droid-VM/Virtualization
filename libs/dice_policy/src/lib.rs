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
//!
//! Dice Policy CDDL:
//!
//! certificateConstraintList = [
//!     * certificateConstraint
//! ]
//!
//! ; We may add a hashConstraint item later
//! certificateConstraint = exactMatchConstraint / geConstraint
//!
//! exactMatchConstraint = [1, keySpec, value]
//! geConstraint = [2, keySpec, int]
//!
//! keySpec = [value+]
//!
//! value = bool / int / tstr / bstr

use anyhow::{anyhow, bail, Context, Result};
use ciborium::{cbor, Value};
use coset::{AsCborValue, CoseKey, CoseSign1, Header, ProtectedHeader};

const DICE_POLICY_VERSION: u64 = 1;
const AUTHORITY_HASH: i64 = -4670549;
const CONFIG_DESC: i64 = -4670548;
const COMPONENT_NAME: i64 = -70002;
const KEY_MODE: i64 = -4670551;

const CONSTRAINT_IDENTIFIER_EXACT_MATCH: i64 = 1;
const CONSTRAINT_IDENTIFIER_GREATER_OR_EQUAL: i64 = 2;

// TODO(b/291238565): Restrict (nested_)key & value type to (bool/int/tstr/bstr).
#[derive(Debug, PartialEq)]
struct NodeConstraints(Box<[(i64, Vec<i64>, Value)]>);

/// Module for working with dice policy.
#[derive(Debug, PartialEq)]
pub struct DicePolicy {
    version: u64,
    node_constraints_list: Box<[NodeConstraints]>, // Constraint on each entry in dice chain.
}

impl DicePolicy {
    /// Construct a dice policy from a given dice chain.
    /// This can be used by clients to construct a policy to seal secrets.
    /// Constraints on all but first node is applied using keys_to_copy argument.
    /// For the first node (which is a ROT key), the constraint is ExactMatch of the whole node.
    ///
    /// # Arguments
    /// `dice_chain`: The serialized CBOR encoded Dice chain, adhering to Android Profile for DICE.
    /// https://pigweed.googlesource.com/open-dice/+/refs/heads/main/docs/android.md
    ///
    /// `keys_to_copy`: List of keys that specify which constraint to apply
    /// and on which entry of the dice node.
    /// Each key is a an array of integer, where first item is CONSTRAINT_IDENTIFIER,
    /// followed by the list of integer to lookup the value in node (in nested fashion)
    ///
    /// The constraint is applied to all of the dice nodes (except the first one).
    ///
    /// Examples of keys_to_copy:
    ///  1. For exact_match on auth_hash & greater_or_equal on security_version
    ///    keys_to_copy =[
    ///     vec![CONSTRAINT_IDENTIFIER_EXACT_MATCH, AUTHORITY_HASH],
    ///     vec![CONSTRAINT_IDENTIFIER_GREATER_OR_EQUAL, CONFIG_DESC, COMPONENT_NAME],
    ///    ];
    ///
    /// 2. For hypothetical (and highly simplified) dice chain:
    ///    [ROT_KEY, [{1 : 'a', 2 : {200 : 5, 201 : 'b'}}]]
    ///    The following can be used
    ///    keys_to_copy =[
    ///     vec![CONSTRAINT_IDENTIFIER_EXACT_MATCH, 1],             // exact_matches value 'a'
    ///     vec![CONSTRAINT_IDENTIFIER_GREATER_OR_EQUAL, 2, 200],   // matches any value >= 5
    ///    ];
    pub fn from_dice_chain(dice_chain: &[u8], keys_to_copy: &[Vec<i64>]) -> Result<Self> {
        // TODO(b/298217847): Check if the given dice chain adheres to Explicit-key DiceCertChain
        // format and if not, convert it before policy construction.
        let dice_chain = value_from_bytes(dice_chain).context("Unable to decode top-level CBOR")?;
        let dice_chain = match dice_chain {
            Value::Array(array) if array.len() >= 2 => array,
            _ => bail!("Expected an array of at least length 2, found: {:?}", dice_chain),
        };
        let mut constraints_list: Vec<NodeConstraints> = Vec::new();
        let mut it = dice_chain.into_iter();

        constraints_list.push(NodeConstraints(Box::new([(
            CONSTRAINT_IDENTIFIER_EXACT_MATCH,
            Vec::new(),
            it.next().unwrap(),
        )])));

        for (n, value) in it.enumerate() {
            let entry = cbor_value_from_cose_sign(value)
                .with_context(|| format!("Unable to get Cose payload at: {}", n))?;
            constraints_list.push(payload_to_constraints(entry, keys_to_copy)?);
        }

        Ok(DicePolicy {
            version: DICE_POLICY_VERSION,
            node_constraints_list: constraints_list.into_boxed_slice(),
        })
    }
}

// Take the payload of a dice node & construct the constraints on it.
fn payload_to_constraints(payload: Value, keys_to_copy: &[Vec<i64>]) -> Result<NodeConstraints> {
    let mut cert_constraints: Vec<(i64, Vec<i64>, Value)> = Vec::new();
    for key in keys_to_copy {
        if key.len() < 2 {
            bail!("Expected at least 2 values in key_constraint, found {}", key.len());
        }
        // The first item is CONSTRAINT_IDENTIFIER, followed by followed by
        // the list of integer to lookup the value in node.
        let constraint = key[0];
        let key_spec = &key[1..];
        let val = lookup_value_in_nested_map(&payload, key_spec)?;
        let constraint = match constraint {
            CONSTRAINT_IDENTIFIER_EXACT_MATCH => {
                (CONSTRAINT_IDENTIFIER_EXACT_MATCH, key_spec.to_vec(), val.clone())
            }
            CONSTRAINT_IDENTIFIER_GREATER_OR_EQUAL => {
                (CONSTRAINT_IDENTIFIER_GREATER_OR_EQUAL, key_spec.to_vec(), val)
            }
            _ => bail!("Unknown constraint type"),
        };
        cert_constraints.push(constraint);
    }
    Ok(NodeConstraints(cert_constraints.into_boxed_slice()))
}

fn lookup_value_in_nested_map(map: &Value, key_path: &[i64]) -> Result<Value> {
    if key_path.is_empty() {
        return Ok(map.clone());
    }
    // Shadowing the original map which is immutable, this allows
    // easy iteration into the nested map.
    let mut map = map.clone();
    for key in key_path.iter() {
        let explicit_map = get_map_from_value(&map)?;
        map = lookup_value_in_map(explicit_map, key)
            .ok_or(anyhow!("Could not find value in map for nested key, {:?}", key))?;
    }
    Ok(map)
}

fn get_map_from_value(val: &Value) -> Result<Vec<(Value, Value)>> {
    match val {
        Value::Bytes(b) => Ok(value_from_bytes(b)?
            .into_map()
            .map_err(|e| anyhow!("Expected a cbor map: {:?}", e))?),
        Value::Map(map) => Ok(map.clone()),
        _ => bail!("Expected a cbor map {:?}", val),
    }
}

fn lookup_value_in_map(map: Vec<(Value, Value)>, key: &i64) -> Option<Value> {
    for (k, v) in map.into_iter() {
        if k.is_integer() && k.into_integer().unwrap() == (*key).into() {
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
    // Ciborium tries to read one Value, but doesn't care if there is trailing data after it. We do.
    if !bytes.is_empty() {
        bail!("Unexpected trailing data while converting to CBOR value");
    }
    Ok(value)
}

/// Encodes a ciborium::Value into bytes.
fn value_to_bytes(value: &Value) -> Result<Vec<u8>> {
    let mut bytes: Vec<u8> = Vec::new();
    ciborium::ser::into_writer(&value, &mut bytes)?;
    Ok(bytes)
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
    const EXAMPLE_STRING: &str = "testing_dice_policy";
    const EXAMPLE_NUM: i64 = 59765;

    test!(policy_dice_size_is_same);
    fn policy_dice_size_is_same() {
        let input_dice = include_bytes!("../testdata/composbcc");
        let keys_to_copy = [
            vec![CONSTRAINT_IDENTIFIER_EXACT_MATCH, AUTHORITY_HASH],
            vec![CONSTRAINT_IDENTIFIER_EXACT_MATCH, KEY_MODE],
            vec![CONSTRAINT_IDENTIFIER_GREATER_OR_EQUAL, CONFIG_DESC, COMPONENT_NAME],
        ];
        let policy = DicePolicy::from_dice_chain(input_dice, &keys_to_copy).unwrap();
        assert_eq!(policy.node_constraints_list.len(), COMPOS_DICE_CHAIN_SIZE);
    }

    test!(policy_structure_check);
    fn policy_structure_check() {
        let rot_key = CoseKey::default().to_cbor_value().unwrap();
        let nested_payload = cbor!({
            100 => EXAMPLE_NUM
        })
        .unwrap();
        let payload = cbor!({
            1 => EXAMPLE_STRING,
            2 => "some_other_example_string",
            3 => Value::Bytes(value_to_bytes(&nested_payload).unwrap()),
        })
        .unwrap();
        let payload = value_to_bytes(&payload).unwrap();
        let dice_node = CoseSign1 {
            protected: ProtectedHeader::default(),
            unprotected: Header::default(),
            payload: Some(payload),
            signature: b"ddef".to_vec(),
        }
        .to_cbor_value()
        .unwrap();
        let input_dice = Value::Array([rot_key.clone(), dice_node].to_vec());

        let input_dice = value_to_bytes(&input_dice).unwrap();
        let keys_to_copy = [
            vec![CONSTRAINT_IDENTIFIER_EXACT_MATCH, 1],
            vec![CONSTRAINT_IDENTIFIER_GREATER_OR_EQUAL, 3, 100],
        ];
        let policy = DicePolicy::from_dice_chain(&input_dice, &keys_to_copy).unwrap();

        // Assert policy is exactly as expected!
        assert_eq!(
            policy,
            DicePolicy {
                version: 1,
                node_constraints_list: Box::new([
                    NodeConstraints(Box::new([(
                        CONSTRAINT_IDENTIFIER_EXACT_MATCH,
                        vec![],
                        rot_key
                    )])),
                    NodeConstraints(Box::new([
                        (
                            CONSTRAINT_IDENTIFIER_EXACT_MATCH,
                            vec![1],
                            Value::Text(EXAMPLE_STRING.to_string())
                        ),
                        (
                            CONSTRAINT_IDENTIFIER_GREATER_OR_EQUAL,
                            vec![3, 100],
                            Value::from(EXAMPLE_NUM)
                        )
                    ])),
                ])
            }
        );
    }
}
