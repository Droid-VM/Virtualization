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
//! client vm.

use alloc::vec::Vec;
use core::result;
use diced_open_dice::DiceArtifacts;
use service_vm_comm::{Csr, RequestProcessingError};

type Result<T> = result::Result<T, RequestProcessingError>;

pub(super) fn request_certificate(
    _csr: Csr,
    _dice_artifacts: &dyn DiceArtifacts,
) -> Result<Vec<u8>> {
    // TODO(b/278717513): Compare client VM's DICE chain up to pvmfw cert with
    // RKP VM's DICE chain.

    // TODO(b/293871876): Returns the google-rooted certificate chain
    // once all the verification succeeds.
    let res = Vec::new();
    Ok(res)
}
