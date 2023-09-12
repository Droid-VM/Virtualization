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
//! service VM via the RKP (Remote Key Provisionning) server.

use super::keys::EcKey;
use crate::error::Result;
use alloc::vec::Vec;
use service_vm_comm::{EcdsaP256KeyPair, GenerateCertificateRequestParams};

pub(super) fn generate_ecdsa_p256_key_pair() -> Result<EcdsaP256KeyPair> {
    let ec_key = EcKey::new_p256()?;
    // TODO(b/279425980): Encrypt the private key in a key blob.
    let public_key = ec_key.public_key()?;
    let public_key = public_key.as_slice();
    // TODO(b/300068317): Build MACed public key.
    let key_pair = EcdsaP256KeyPair { maced_public_key: public_key.to_vec(), key_blob: Vec::new() };
    Ok(key_pair)
}

pub(super) fn generate_certificate_request(
    _params: GenerateCertificateRequestParams,
) -> Result<Vec<u8>> {
    // TODO(b/299256925): Generate the certificate request
    Ok(Vec::new())
}
