/*
 * Copyright (C) 2021 The Android Open Source Project
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

use apkverify::{testing::assert_contains, verify};
use std::matches;

#[test]
fn test_verify_v3() {
    assert!(verify("tests/data/test.apex").is_ok());
}

#[test]
fn test_verify_v3_dsa_sha256_1024() {
    // TODO(b/190343842)
    let res = verify("tests/data/v3-only-with-dsa-sha256-1024.apk");
    assert!(res.is_err());
    assert_contains(
        &res.unwrap_err().to_string(),
        "TODO(b/190343842) not implemented signature algorithm",
    );
}

#[test]
fn test_verify_v3_ecdsa_sha256_p256() {
    assert!(verify("tests/data/v3-only-with-ecdsa-sha256-p256.apk").is_ok());
}

#[test]
fn test_verify_v3_ecdsa_sha512_p256() {
    // TODO(b/190343842)
    let res = verify("tests/data/v3-only-with-ecdsa-sha512-p256.apk");
    assert!(res.is_err());
    assert_contains(
        &res.unwrap_err().to_string(),
        "TODO(b/190343842) not implemented signature algorithm",
    );
}

#[test]
fn test_verify_v3_rsa_pkcs1_sha256_3072() {
    assert!(verify("tests/data/v3-only-with-rsa-pkcs1-sha256-3072.apk").is_ok());
}

#[test]
fn test_verify_v3_rsa_pkcs1_sha512_4096() {
    assert!(verify("tests/data/v3-only-with-rsa-pkcs1-sha512-4096.apk").is_ok());
}

#[test]
fn test_verify_v3_digest_mismatch() {
    let res = verify("tests/data/v3-only-with-rsa-pkcs1-sha512-8192-digest-mismatch.apk");
    assert!(res.is_err());
    assert_contains(&res.unwrap_err().to_string(), "Digest mismatch");
}

#[test]
fn test_verify_v3_cert_and_public_key_mismatch() {
    let res = verify("tests/data/v3-only-cert-and-public-key-mismatch.apk");
    assert!(res.is_err());
    assert_contains(&res.unwrap_err().to_string(), "Public key mismatch");
}

#[test]
fn test_verify_truncated_cd() {
    use zip::result::ZipError;
    let res = verify("tests/data/v2-only-truncated-cd.apk");
    // TODO(jooyung): consider making a helper for err assertion
    assert!(matches!(
        res.unwrap_err().root_cause().downcast_ref::<ZipError>().unwrap(),
        ZipError::InvalidArchive(_),
    ));
}

#[test]
fn test_verify_v3_empty() {
    let res = verify("tests/data/v3-only-empty.apk");
    assert!(res.is_err());
    assert_contains(&res.unwrap_err().to_string(), "APK too small for APK Signing Block");
}

#[test]
fn test_verify_v3_wrong_apk_sig_block_magic() {
    let res = verify("tests/data/v3-only-with-ecdsa-sha512-p384-wrong-apk-sig-block-magic.apk");
    assert!(res.is_err());
    assert_contains(&res.unwrap_err().to_string(), "No APK Signing Block");
}

#[test]
fn test_verify_v3_apk_sig_block_size_mismatch() {
    let res =
        verify("tests/data/v3-only-with-rsa-pkcs1-sha512-4096-apk-sig-block-size-mismatch.apk");
    assert!(res.is_err());
    assert_contains(
        &res.unwrap_err().to_string(),
        "APK Signing Block sizes in header and footer do not match",
    );
}

#[test]
fn test_verify_v3_no_certs_in_sig() {
    let res = verify("tests/data/v3-only-no-certs-in-sig.apk");
    assert!(res.is_err());
    assert_contains(&res.unwrap_err().to_string(), "No certificates listed");
}

#[test]
fn test_verify_v3_no_supported_sig_algs() {
    let res = verify("tests/data/v3-only-no-supported-sig-algs.apk");
    assert!(res.is_err());
    assert_contains(&res.unwrap_err().to_string(), "No supported signatures found");
}

#[test]
fn test_verify_v3_signatures_and_digests_block_mismatch() {
    let res = verify("tests/data/v3-only-signatures-and-digests-block-mismatch.apk");
    assert!(res.is_err());
    assert_contains(
        &res.unwrap_err().to_string(),
        "Signature algorithms don't match between digests and signatures records",
    );
}

#[test]
fn test_verify_v3_sig_does_not_verify() {
    let res = verify("tests/data/v3-only-with-rsa-pkcs1-sha256-3072-sig-does-not-verify.apk");
    assert!(res.is_err());
    assert_contains(&res.unwrap_err().to_string(), "Signature is invalid");
}

#[test]
fn test_verify_v3_unknown_additional_attr() {
    // TODO
    assert!(verify("tests/data/v3-only-unknown-additional-attr.apk").is_ok());
}

#[test]
fn test_verify_v3_unknown_pair_in_apk_sig_block() {
    // TODO
    assert!(verify("tests/data/v3-only-unknown-pair-in-apk-sig-block.apk").is_ok());
}

#[test]
fn test_verify_v3_ignorable_unsupported_sig_algs() {
    // TODO
    assert!(verify("tests/data/v3-only-with-ignorable-unsupported-sig-algs.apk").is_ok());
}

#[test]
fn test_verify_v3_stamp() {
    // TODO
    assert!(verify("tests/data/v3-only-with-stamp.apk").is_ok());
}
