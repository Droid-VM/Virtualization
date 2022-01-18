/*
 * Copyright 2022 The Android Open Source Project
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

//! Allows for data to be encrypted and authenticated (AEAD) with a key derived from some secret.
//! The encrypted blob can be passed to the untrusted host without revealing the encrypted data
//! but with the key the data can be retrieved as long as the blob has not been tampered with.

use anyhow::{anyhow, Context, Result};
use ring::{
    aead::{
        Aad, Algorithm, BoundKey, Nonce, NonceSequence, OpeningKey, SealingKey, UnboundKey,
        AES_128_GCM, NONCE_LEN,
    },
    error::Unspecified,
    hkdf::{Salt, HKDF_SHA256},
    rand::{SecureRandom, SystemRandom},
};

pub struct BlobEncryptor {
    random: SystemRandom,
}

static AEAD_ALGORITHM: &Algorithm = &AES_128_GCM;

// Non-secret input to the AEAD key derivation
const KDF_INFO: &[u8] = b"CompOS blob sealing key";

impl BlobEncryptor {
    pub fn new() -> Self {
        Self { random: SystemRandom::new() }
    }

    pub fn derive_aead_key(&self, input_keying_material: &[u8]) -> Result<UnboundKey> {
        // Derive key using HKDF - see https://datatracker.ietf.org/doc/html/rfc5869#section-2
        let salt = [];
        let prk = Salt::new(HKDF_SHA256, &salt).extract(input_keying_material);
        let okm = prk.expand(&[KDF_INFO], AEAD_ALGORITHM).context("HKDF failed")?;
        Ok(okm.into())
    }

    pub fn encrypt_bytes(&self, aead_key: UnboundKey, mut bytes: Vec<u8>) -> Result<Vec<u8>> {
        // Generate a unique nonce, since we may use the same key more than once.
        let mut nonce_bytes = [0u8; NONCE_LEN];
        self.random.fill(&mut nonce_bytes).context("Failed to generate random nonce")?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        // Encrypt & seal the data in place.
        let nonce_sequence = SingleNonceSequence { nonce: Some(nonce) };
        let mut key = SealingKey::new(aead_key, nonce_sequence);
        key.seal_in_place(Aad::empty(), &mut bytes).context("Failed to seal blob")?;

        // Append the nonce since we'll need it to decrypt.
        bytes.extend(&nonce_bytes);

        Ok(bytes)
    }

    pub fn decrypt_bytes(&self, aead_key: UnboundKey, mut bytes: Vec<u8>) -> Result<Vec<u8>> {
        // Remove the nonce from the end of the blob
        let encryted_size = bytes
            .len()
            .checked_sub(NONCE_LEN)
            .ok_or_else(|| anyhow!("Encrypted blob is too small"))?;
        let nonce_bytes = &bytes[encryted_size..];
        let nonce = Nonce::try_assume_unique_for_key(nonce_bytes).unwrap();
        bytes.truncate(encryted_size);

        // Verify & decrypt the data in place
        let nonce_sequence = SingleNonceSequence { nonce: Some(nonce) };
        let mut key = OpeningKey::new(aead_key, nonce_sequence);
        let data_len =
            key.open_in_place(Aad::empty(), &mut bytes).context("Failed to unseal blob")?.len();

        // Remove the extra authentication data after the plaintext
        bytes.truncate(data_len);

        Ok(bytes)
    }
}

struct SingleNonceSequence {
    nonce: Option<Nonce>,
}

impl NonceSequence for SingleNonceSequence {
    fn advance(&mut self) -> Result<Nonce, Unspecified> {
        self.nonce.take().ok_or(Unspecified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip_data() -> Result<()> {
        let encryptor = BlobEncryptor::new();
        let input_keying_material = b"Key is derived from this";
        let original_bytes = b"This is the secret data";

        let key = encryptor.derive_aead_key(input_keying_material)?;
        let blob = encryptor.encrypt_bytes(key, original_bytes.to_vec())?;

        let key = encryptor.derive_aead_key(input_keying_material)?;
        let decoded_bytes = encryptor.decrypt_bytes(key, blob)?;

        assert_eq!(decoded_bytes, original_bytes);
        Ok(())
    }

    #[test]
    fn test_modified_data_detected() -> Result<()> {
        let encryptor = BlobEncryptor::new();
        let input_keying_material = b"Key is derived from this";
        let original_bytes = b"This is the secret data";

        let key = encryptor.derive_aead_key(input_keying_material)?;
        let mut blob = encryptor.encrypt_bytes(key, original_bytes.to_vec())?;

        // Flip a bit.
        blob[0] ^= 1;

        let key = encryptor.derive_aead_key(input_keying_material)?;
        let decoded_bytes = encryptor.decrypt_bytes(key, blob);

        assert!(decoded_bytes.is_err());
        Ok(())
    }
}
