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

//! TODO

pub use ciborium::Value;

use crate::cbor_convert::{value_from_bytes, value_to_bytes, value_to_integer};
use crate::data_types::error::Error;
use crate::data_types::error::ERROR_OK;
use crate::data_types::request_response_impl::Opcode;
use alloc::vec::Vec;

/// TODO
// #[derive(Clone, Debug, PartialEq)]
pub struct ProtectedRequestPacket(Vec<Value>);

// #[derive(Clone, Debug, PartialEq)]
pub struct ProtectedResponsePacket(Vec<Value>);

/// Encapsulate cryptographically protected payload.
struct CryptoPayload(CoseEncrypt0);

impl CryptoPayload {
    // TODO, either way increment the connection artifacts
    /// CryptoPayload<Payload, Key> = [         ; COSE_Encrypt0 (untagged), [RFC 9052 s5.2]
    ///     protected: bstr .cbor {
    ///         1 : 3,                  ; Algorithm: AES-GCM mode w/ 256-bit key, 128-bit tag
    ///         4 : bstr                ; key identifier, uniquely identifies the session
    ///                                 ; TODO(b/291228560): Refer to the Key Exchange spec.
    ///     },
    ///     unprotected: {
    ///         5 : bstr .size 12          ; IV
    ///     },
    ///     ciphertext : bstr     ; AES-GCM-256(Key, bstr .cbor Payload)
    ///                         ; AAD for the encryption is CBOR-serialized
    ///                         ; Enc_structure (RFC 9052 s5.3) with empty external_aad.
    /// ]
    fn from(payload: Value, connection_artifacts: &ConnectionArtifacts, aes: &A) -> Result<Self, Error> {
        let mut protected_hdr = HeaderBuilder::new().algorithm(iana::Algorithm::A256GCM);
        let mut unprotected_hdr =
            HeaderBuilder::new().unprotected_hdr.iv(try_to_vec(&nonce_for_enc.0)?);

        let cose_encrypt = coset::CoseEncrypt0Builder::new()
            .protected(protected_hdr.build())
            .try_create_ciphertext::<_, Error>(value_to_bytes(payload), &[], |pt, aad| {
                let ct = BoringAes {}
                    .encrypt(&KEY, &packet.clone().into_bytes().unwrap(), aad, &nonce)
                    .unwrap();
                aes
                    .encrypt(
                        connection_artifacts.encrypt_key,
                        &packet.clone().into_bytes().unwrap(),
                        aad,
                        &connection_artifacts.nonce.get_and_bump()?,
                    )
                    .unwrap();
            })?
            .build();
        Ok(Self(cose_encrypt))
    }

    fn decrypt_from(&self, connection_artifacts: &ConnectionArtifacts, aes: &A) -> Result<Value, Error> {
        //     let mut encrypt = CoseEncryptBuilder::new()
        //     .protected(protected)
        //     .create_ciphertext(pt, external_aad, |pt, aad| cipher.encrypt(pt, aad).unwrap())
        //     .build();

        // let recovered_pt = encrypt
        //     .decrypt(external_aad, |ct, aad| cipher.decrypt(ct, aad))
        //     .unwrap();
        // assert_eq!(&pt[..], recovered_pt);
        let 
    }
}

// Can be constructed artifacts
struct ConnectionArtifacts {
    // in_encrypt_key: AesKey,
    encrypt_key: AesKey,
    nonce: NonceSequence, // replay counter
            // in_iv_min: IV,
            // hmac key
            // Session id & all required?
            // AesKey(in_encrypt_key.0),
}

impl ConnectionArtifacts {
    fn encrypt_key(&self) -> AesKey {
        self.encrypt_key
    }
    fn nonce(&mut self) -> &mut NonceSequence {
        self.nonce
    }
}

/// NIST SP 800-38D recommends The total number of invocations of the authenticated encryption
/// function shall not exceed 2^32. TODO how to restrict this.
#[derive(Clone)]
pub struct NonceSequence(u32);
impl NonceSequence {
    pub fn new() -> Self {
        Self(0)
    }
    /// Create a random nonce of 16 bytes
    pub fn get_and_bump(&mut self) -> Result<[u8; 12], Error> {
        // The first 4 bytes compose the counter, 0 padding is added  
        let mut result = [0u8; 12];
        result[..4].copy_from_slice(&u32::to_le_bytes(self.0));
        self.0 = self.0.checked_add(1).ok_or(Error::NonceSequenceOverflow)?;
        Ok(result)
    }
}