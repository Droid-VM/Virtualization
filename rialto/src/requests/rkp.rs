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

//! This module contains functions related to the attestation of the
//! service VM via the RKP (Remote Key Provisioning) server.

use super::ec_key::EcKey;
use super::pub_key::build_maced_public_key;
use crate::error::Result;
use alloc::vec::Vec;
use service_vm_comm::{EcdsaP256KeyPair, GenerateCertificateRequestParams};

pub(super) fn generate_ecdsa_p256_key_pair() -> Result<EcdsaP256KeyPair> {
    let ec_key = EcKey::new_p256()?;
    let maced_public_key = build_maced_public_key(ec_key.cose_public_key()?)?;

    // TODO(b/279425980): Encrypt the private key in a key blob.
    let key_blob = ec_key.private_key()?.as_slice().to_vec();

    let key_pair = EcdsaP256KeyPair { maced_public_key, key_blob };
    Ok(key_pair)
}

pub(super) fn generate_certificate_request(
    _params: GenerateCertificateRequestParams,
) -> Result<Vec<u8>> {
    // TODO(b/299256925): Generate the certificate request
    Ok(Vec::new())
}
