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
use secretkeeper_comm::data_types::error::{Error, SecretkeeperError, StorageError};
use secretkeeper_comm::data_types::types::{Id, Secret};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Entry {
    secret: Secret,
    sealing_policy: Vec<u8>, // dice policy serialized into bytes
}

/// This trait defines behavior of the storage layer of Secretkeeper, which in addition to
/// conventional storage, provides Authentication - ie, a client can restrict the access to it's
/// stored entry.
///
/// * Storage: SecretkeeperStore allows storing a Secret (and authentication data) which is indexed
/// by an Id. Under the hood, it uses a Key-Value based storage, which should be provided on
/// initialization.
/// The security properties (confidentiality/Integrity/Persistence) expected from the Storage are
/// listed in ISecretkeeper.aidl
///
/// * Authentication: Secretkeeper uses Dice policy based authentication. Each secret is associated
/// with sealing_policy, which is a dice policy. This is a required input while storing a secret.
/// Further access to this secret is restricted to clients whose dice chain adhered to the
/// sealing_policy.
pub struct SecretkeeperStore {
    secure_store: Box<dyn KeyValStore>
}
impl SecretkeeperStore {
    fn init(store : Box<dyn KeyValStore>) -> Self {
        Self{store}
    }
    /// Store a secret.
    ///
    /// # Arguments
    /// `id`: Unique identifier of the secret. A client is allowed to have multiple entries each
    /// with a distinct id.
    /// If an entry corresponding to id is already present AND dice_chain matches the
    /// (already present) sealing_policy -> update the corresponding `secret` & `sealing_policy`.
    ///
    /// `secret`: The secret the client wishes to store.
    ///
    /// `sealing_policy`: The dice policy corresponding to the secret. Only clients with dice chain
    /// with dice chain which matches the sealing_policy are allowed to access Secret.
    ///
    /// `dice_chain`: The serialized CBOR encoded Dice chain of the client, adhering to
    /// Android Profile for DICE.
    /// https://pigweed.googlesource.com/open-dice/+/refs/heads/main/docs/android.md
    /// The caller must ensure that the dice chain indeed belongs to client. This can be done by
    /// having client send a signed dice chain & verifying it. This method verifies that the
    /// dice_chain matches the provided sealing_policy.
    fn store(
        &self,
        id: Id,
        secret: Secret,
        sealing_policy: Vec<u8>,
        dice_chain: &[u8],
    ) -> Result<(), SecretkeeperError> {
        // Check if an entry for the id is already present & if so, whether the dice_chain matches
        // the already present sealing_policy.
        match SecretkeeperStore::get(self, &id, dice_chain) {
            Ok(..) => {
                info!("Found an existing entry, authentication succeeded, updating the secret");
            }
            Err(SecretkeeperError::EntryNotFound) => {
                info!("No existing entry, attempting to create a new entry..");
            }
            Err(e) => {
                info!("There may have been an existing entry, but reading it failed {}", e);
                return Err(e);
            }
        }

        // Sanity check the dice_chain matches the sealing_policy on the secret it is trying to
        // store. This ensures client can not store a secret that it cannot access itself.
        // Such requests are considered malformed.
        authenticate_against_dice_policy(dice_chain, &sealing_policy)
            .map_err(|_| SecretkeeperError::RequestMalformed)?;

        let entry = Entry { secret, sealing_policy };

        self.0.store(
            id,
            serde_cbor::to_vec(&entry)?, // .map_err(|_| SecretkeeperError::SerializationError)?, // TODO map it directly by implementing From trait
        )
        .map_err(|_| SecretkeeperError::UnexpectedServerError) // TODO: map to precise error
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
    /// `updated_sealing_policy`: The updated dice_policy corresponding to the secret.
    /// This is an optional parameter and can be used to update the sealing_policy associated with
    /// the secret.
    fn get(
        &self,
        id: &Id,
        dice_chain: &[u8],
        updated_sealing_policy: Option<Vec<u8>>,
    ) -> Result<Secret, SecretkeeperError> {
        // TODO: Can we get anymore specific for the returned error
        match self.0.get(id).map_err(|_| SecretkeeperError::UnexpectedServerError)? {
            Some(entry_serialized) => {
                let entry: Entry = serde_cbor::from_slice(&entry_serialized)
                    .map_err(|_| SecretkeeperError::SerializationError)?;
                authenticate_against_dice_policy(dice_chain, &entry.sealing_policy)
                    .map_err(|_| SecretkeeperError::DicePolicyError)?;

                // Update the entry with updated_sealing_policy if present.
                if let Some(updated_sealing_policy) = updated_sealing_policy {
                    authenticate_against_dice_policy(dice_chain, &updated_sealing_policy)
                        .map_err(|_| SecretkeeperError::DicePolicyError)?;
                    let new_entry =
                        Entry { secret: entry.secret, sealing_policy: updated_sealing_policy };
                        self.0.store(id, serde_cbor::to_vec(&new_entry)?)
                        .map_err(|_| SecretkeeperError::UnexpectedServerError) // TODO: map to precise error
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
pub trait KeyValStore {
    /// TODO: Overwrites if not already present
    fn store(&self, key: Vec<u8>, val: Vec<u8>) -> Result<(), StorageError>;
    /// TODO
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StorageError>;
}
