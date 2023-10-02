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

//! Handles the encryption and decryption of the key blob.

use alloc::vec;
use alloc::vec::Vec;
use bssl_avf::{hkdf, Aead, AeadCtx, Digester, AES_GCM_NONCE_LENGTH};
use core::result;
use log::error;
use serde::{Deserialize, Serialize};
use service_vm_comm::RequestProcessingError;
use vmbase::rand;

type Result<T> = result::Result<T, RequestProcessingError>;

const KEK_INFO: &[u8] = b"rialto keyblob kek";

/// An all-zero nonce is utilized to encrypt the private key. This is because each key
/// undergoes encryption using a distinct KEK, which is derived from a secret and a random
/// salt. Since the uniqueness of the IV/key combination is already guaranteed by the uniqueness
/// of the KEK, there is no need for an additional random nonce.
const PRIVATE_KEY_NONCE: &[u8; AES_GCM_NONCE_LENGTH] = &[0; AES_GCM_NONCE_LENGTH];

/// Since Rialto functions as both the sender and receiver of the message, no additional data is
/// needed.
const PRIVATE_KEY_AD: &[u8] = &[];

// Encrypted key blob.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum EncryptedKeyBlob {
    /// Version 1 key blob.
    V1(EncryptedKeyBlobV1),
}

/// Encrypted key blob version 1.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct EncryptedKeyBlobV1 {
    /// Salt used to derive the KEK.
    kek_salt: [u8; 32],

    /// Private key encrypted with AES-256-GCM.
    encrypted_private_key: Vec<u8>,
}

impl EncryptedKeyBlob {
    pub(super) fn new(private_key: &[u8], kek_secret: &[u8]) -> Result<Self> {
        let kek_salt = rand::random_array().map_err(|e| {
            error!("Failed to generate the salt for KEK: {e:?}");
            RequestProcessingError::RandomArrayGenerationFailed
        })?;
        let kek = hkdf::<32>(kek_secret, &kek_salt, KEK_INFO, Digester::sha512())?;

        let tag_len = None;
        let aead_ctx = AeadCtx::new(Aead::aes_256_gcm(), kek.as_slice(), tag_len)?;
        let mut out = vec![0u8; private_key.len() + aead_ctx.aead().max_overhead()];
        let ciphertext = aead_ctx.seal(private_key, PRIVATE_KEY_NONCE, PRIVATE_KEY_AD, &mut out)?;

        let key_blob = EncryptedKeyBlobV1 { kek_salt, encrypted_private_key: ciphertext.to_vec() };
        Ok(Self::V1(key_blob))
    }

    // TODO(b/303225858): Decrypt the private key from the keyblob with a given `kek_secret`.
}
