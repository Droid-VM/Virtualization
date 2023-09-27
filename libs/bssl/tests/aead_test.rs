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

use bssl_avf::{Aead, AeadCtx, ApiName, Error, Result, AEAD_DEFAULT_TAG_LENGTH};

const KEY1: [u8; 32] = [
    0xdb, 0x16, 0xcc, 0xbf, 0xf0, 0xc4, 0xbc, 0x93, 0xc3, 0x5f, 0x11, 0xc5, 0xfa, 0xae, 0x03, 0x6c,
    0x75, 0x40, 0x1f, 0x60, 0xb6, 0x3e, 0xb9, 0x2a, 0x6c, 0x84, 0x06, 0x4b, 0x36, 0x7f, 0xed, 0xdb,
];
const KEY2: [u8; 32] = [
    0xaa, 0x57, 0x7a, 0x1a, 0x8b, 0xa2, 0x59, 0x3b, 0xad, 0x5f, 0x4d, 0x29, 0xe1, 0x0c, 0xaa, 0x85,
    0xde, 0xf9, 0xad, 0xad, 0x8c, 0x11, 0x0c, 0x2e, 0x13, 0x43, 0xd7, 0xdf, 0x2a, 0x43, 0xb9, 0xdd,
];
const MESSAGE: &[u8] = b"aead_aes_256_gcm test message";

#[test]
fn aes_256_gcm_randnonce_encrypts_and_decrypts_successfully() -> Result<()> {
    let aead_ctx = AeadCtx::new(Aead::aes_256_gcm_randnonce(), &KEY1, AEAD_DEFAULT_TAG_LENGTH)?;
    let mut ciphertext = vec![0u8; MESSAGE.len() + aead_ctx.aead().max_overhead()];
    let nonce = &[];
    let ad = &[];

    let ciphertext_len = aead_ctx.seal(MESSAGE, nonce, ad, &mut ciphertext)?;
    ciphertext.truncate(ciphertext_len);
    assert!(MESSAGE != ciphertext);

    // Decryption.
    let mut plaintext = vec![0u8; ciphertext_len];
    let plaintext_len = aead_ctx.open(&ciphertext, nonce, ad, &mut plaintext)?;
    plaintext.truncate(plaintext_len);

    assert_eq!(MESSAGE, plaintext);
    Ok(())
}

#[test]
fn aes_256_gcm_randnonce_fails_to_decrypt_with_wrong_key() -> Result<()> {
    let aead_ctx = AeadCtx::new(Aead::aes_256_gcm_randnonce(), &KEY1, AEAD_DEFAULT_TAG_LENGTH)?;
    let mut ciphertext = vec![0u8; MESSAGE.len() + aead_ctx.aead().max_overhead()];
    let nonce = &[];
    let ad = &[];
    let ciphertext_len = aead_ctx.seal(MESSAGE, nonce, ad, &mut ciphertext)?;
    ciphertext.truncate(ciphertext_len);

    // Decryption.
    let mut plaintext = vec![0u8; ciphertext_len];
    let aead_ctx2 = AeadCtx::new(Aead::aes_256_gcm_randnonce(), &KEY2, AEAD_DEFAULT_TAG_LENGTH)?;
    let res = aead_ctx2.open(&ciphertext, nonce, ad, &mut plaintext);

    assert_eq!(res, Err(Error::CallFailed(ApiName::EVP_AEAD_CTX_open)));
    Ok(())
}

#[test]
fn aes_256_gcm_randnonce_fails_to_decrypt_with_different_ad() -> Result<()> {
    let aead_ctx = AeadCtx::new(Aead::aes_256_gcm_randnonce(), &KEY1, AEAD_DEFAULT_TAG_LENGTH)?;
    let mut ciphertext = vec![0u8; MESSAGE.len() + aead_ctx.aead().max_overhead()];
    let nonce = &[];
    let ad = &[];
    let ciphertext_len = aead_ctx.seal(MESSAGE, nonce, ad, &mut ciphertext)?;
    ciphertext.truncate(ciphertext_len);

    // Decryption.
    let mut plaintext = vec![0u8; ciphertext_len];
    let ad2 = &[1];
    let res = aead_ctx.open(&ciphertext, nonce, ad2, &mut plaintext);

    assert_eq!(res, Err(Error::CallFailed(ApiName::EVP_AEAD_CTX_open)));
    Ok(())
}

#[test]
fn aes_256_gcm_randnonce_fails_to_decrypt_corrupted_ciphertext() -> Result<()> {
    let aead_ctx = AeadCtx::new(Aead::aes_256_gcm_randnonce(), &KEY1, AEAD_DEFAULT_TAG_LENGTH)?;
    let mut ciphertext = vec![0u8; MESSAGE.len() + aead_ctx.aead().max_overhead()];
    let nonce = &[];
    let ad = &[];
    let ciphertext_len = aead_ctx.seal(MESSAGE, nonce, ad, &mut ciphertext)?;
    ciphertext.truncate(ciphertext_len);
    ciphertext[1] = !ciphertext[1];

    // Decryption.
    let mut plaintext = vec![0u8; ciphertext_len];
    let res = aead_ctx.open(&ciphertext, nonce, ad, &mut plaintext);

    assert_eq!(res, Err(Error::CallFailed(ApiName::EVP_AEAD_CTX_open)));
    Ok(())
}
