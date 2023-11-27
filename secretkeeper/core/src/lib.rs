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

//! This library exposes Authenticated storage and related traits that are useful for
//! Secretkeeper service implementation.
//! This is compatible with Secretkeeper HAL specification.

#![no_std]
extern crate alloc;

use dice_policy::authenticate_against_dice_policy;
use log::info;
use secretkeeper_comm::data_types::cbor_ser::{CborBytesConversion, ValueConversion};
use secretkeeper_comm::data_types::error::{Error, SecretkeeperError};
use secretkeeper_comm::data_types::{Id, Secret};

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use ciborium::Value;

/// SecretkeeperStore encapsulates the storage layer of Secretkeeper, which, in addition to
/// conventional storage, provides Authentication - ie, a client can restrict the access to it's
/// stored entry.
///
/// 1) Storage: SecretkeeperStore allows storing a Secret (and authentication data) which is indexed
/// by an Id. Under the hood, it uses a Key-Value based storage, which should be provided on
/// initialization.
/// The security properties (confidentiality/Integrity/Persistence) expected from the Storage are
/// listed in ISecretkeeper.aidl
///
/// 2) Authentication: Secretkeeper uses Dice policy based authentication. Each secret is associated
/// with sealing_policy, which is a dice policy. This is a required input while storing a secret.
/// Further access to this secret is restricted to clients whose dice chain adhered to the
/// sealing_policy.
pub struct SecretkeeperStore {
    secure_store: Arc<dyn KeyValueStore>,
}
impl SecretkeeperStore {
    /// Initialize the secretkeeper with a Key-Value store. Note: this key-value storage is the
    /// only `persistent` part of Secretkeeper HAL.
    pub fn init(secure_store: Arc<dyn KeyValueStore>) -> Self {
        Self { secure_store }
    }

    /// Store a secret.
    ///
    /// # Arguments
    /// * `id`: Unique identifier of the [`secret`]. A client is allowed to have multiple entries
    ///   each with a distinct `id`. If an entry corresponding to [`id`] is already present AND
    ///   [`dice_chain`] matches the (already present) [`sealing_policy`] -> update the
    ///   corresponding [`Secret`] & its `sealing_policy`.
    ///
    /// * `secret`: The [`Secret`] the client wishes to store.
    ///
    /// * `sealing_policy`: The dice policy corresponding to the secret. Only clients with dice
    ///   chain with dice chain which matches the sealing_policy are allowed to access Secret.
    ///
    /// * `dice_chain`: The serialized CBOR encoded Dice chain of the client, adhering to
    ///    Android Profile for DICE.
    ///    https://pigweed.googlesource.com/open-dice/+/refs/heads/main/docs/android.md
    ///    The caller must ensure that the dice chain indeed belongs to client. This can be done by
    ///    having client send a signed dice chain & verifying it. This method verifies that the
    ///    dice_chain matches the provided sealing_policy but does not authenticate that the dice
    ///    chain indeed belongs to the client.
    pub fn store(
        &self,
        id: Id,
        secret: Secret,
        sealing_policy: Vec<u8>,
        dice_chain: &[u8],
    ) -> Result<(), SecretkeeperError> {
        // Check if an entry for the id is already present & if so, whether the dice_chain matches
        // the already present sealing_policy.
        match self.get(&id, dice_chain, None) {
            Ok(..) => {
                info!("Found an existing entry, authentication succeeded, updating the secret");
            }
            Err(SecretkeeperError::EntryNotFound) => {
                info!("No existing entry, attempting to create a new entry..");
            }
            Err(e) => {
                info!("There may have been an existing entry, but reading it failed {:?}", e);
                return Err(e);
            }
        }

        // Sanity check the dice_chain matches the sealing_policy on the secret it is trying to
        // store. This ensures client can not store a secret that it cannot access itself.
        // Such requests are considered malformed.
        authenticate_against_dice_policy(dice_chain, &sealing_policy)
            .map_err(|_| SecretkeeperError::RequestMalformed)?;

        let entry = Entry { secret, sealing_policy };

        self.secure_store
            .store(
                id.to_vec().map_err(|_| SecretkeeperError::SerializationError)?,
                entry.to_vec().map_err(|_| SecretkeeperError::SerializationError)?,
            )
            .map_err(|_| SecretkeeperError::UnexpectedServerError)?; // TODO: map to precise error
        Ok(())
    }

    /// Get the secret.
    ///
    /// # Arguments
    /// `id`: Unique identifier of the secret.
    ///
    /// `dice_chain`: The serialized CBOR encoded Dice chain of the client, adhering to
    /// Android Profile for DICE.
    /// https://pigweed.googlesource.com/open-dice/+/refs/heads/main/docs/android.md
    /// The caller must ensure that the dice chain indeed belongs to client. This can be done by
    /// having client send a signed dice chain & verifying it. This method verifies that the
    /// dice_chain matches the provided sealing_policy.
    ///
    /// `updated_sealing_policy`: The updated dice_policy corresponding to the [`Secret`].
    ///  This is an optional parameter and can be used to update the sealing_policy associated with
    ///  the [`Secret`].
    pub fn get(
        &self,
        id: &Id,
        dice_chain: &[u8],
        updated_sealing_policy: Option<Vec<u8>>,
    ) -> Result<Secret, SecretkeeperError> {
        match self
            .secure_store
            .get(&id.clone().to_vec().map_err(|_| SecretkeeperError::SerializationError)?)
            .map_err(|_| SecretkeeperError::UnexpectedServerError)?
        {
            Some(entry_serialized) => {
                let entry = Entry::from_slice(&entry_serialized)
                    .map_err(|_| SecretkeeperError::SerializationError)?;
                authenticate_against_dice_policy(dice_chain, &entry.sealing_policy)
                    .map_err(|_| SecretkeeperError::DicePolicyError)?;

                // Update the entry with updated_sealing_policy.
                if let Some(updated_sealing_policy) = updated_sealing_policy {
                    authenticate_against_dice_policy(dice_chain, &updated_sealing_policy)
                        .map_err(|_| SecretkeeperError::DicePolicyError)?;
                    let new_entry = Entry {
                        secret: entry.secret.clone(),
                        sealing_policy: updated_sealing_policy,
                    };
                    self.secure_store
                        .store(
                            id.clone()
                                .to_vec()
                                .map_err(|_| SecretkeeperError::SerializationError)?,
                            new_entry
                                .to_vec()
                                .map_err(|_| SecretkeeperError::SerializationError)?,
                        )
                        // TODO: map to precise error
                        .map_err(|_| SecretkeeperError::UnexpectedServerError)?;
                }
                Ok(entry.secret)
            }
            None => {
                info!("Entry for id: {:?} not found", id);
                Err(SecretkeeperError::EntryNotFound)
            }
        }
    }
}

/// Defines the behavior of a simple Key-Value based storage, where both key & value are bytes.
/// Expected persistence property is dictated by the concrete type implementing the trait.
pub trait KeyValueStore: Send + Sync {
    /// Store a key-value pair. If the key is already present, update the corresponding value.
    fn store(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), Error>;
    /// Get the `value` corresponding to given `key`. Return None if the key is not found.
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Error>;
}

// Entry holds sensitive data. Do not derive debug for it!
struct Entry {
    secret: Secret,
    sealing_policy: Vec<u8>, // dice policy serialized into bytes
}

impl ValueConversion for Entry {
    fn from_cbor_value(val: Value) -> Result<Self, Error> {
        let mut arr = val.into_array()?;
        if arr.len() != 2 {
            return Err(Error::ConversionError);
        }
        let sealing_policy = arr.pop().expect("Vec empty, this is unexpected").into_bytes()?;
        let secret = Secret::from_cbor_value(arr.pop().expect("Vec empty, this is unexpected"))?;
        Ok(Self { secret, sealing_policy })
    }

    fn to_cbor_value(self) -> Value {
        Value::Array(vec![self.secret.to_cbor_value(), Value::from(self.sealing_policy)])
    }
}

impl CborBytesConversion for Entry {}
