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

//! This module contains the structs related to the CSR(Certificate Signing Request)
//! sent from the client VM to the service VM for attestation.

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Represents the data sent from the client VM to the service VM for attestation.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CsrPayload {
    /// A random array with a length between 0 and 64.
    /// It will be included in the certificate chain in the attestation result,
    /// serving as proof of the freshness of the result.
    pub challenge: Vec<u8>,

    /// Public key to be attested.
    pub public_key: Vec<u8>,

    /// The DICE certificate chain of the client VM.
    pub dice_cert_chain: Vec<u8>,
}
