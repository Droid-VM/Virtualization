// Copyright 2022, The Android Open Source Project
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

//! Logic for SecretRetreival of a pVM.

use anyhow::Result;
use diced_open_dice::{DiceArtifacts, OwnedDiceArtifacts};
use keystore2_crypto::ZVec;
use openssl::hkdf::hkdf;
use openssl::md::Md;
// TODO: Re-evaluate what is the right size of rollback protected secrets
const VM_SECRET_SIZE: usize = 32;

/// VM Secrets.
pub enum VmSecret {
    V2 {
        dice: OwnedDiceArtifacts,
        // Replay protected secret.
        // Extend this to encapsulate the SecretKeeper driver for fetching the secrets
        rp_secret: ZVec,
    },
    V1 {
        // V1 secrets are not protected against rollback of boot images.
        // They are reliable only if rollback protection was supported in boot.
        // These are now legacy Secrets.
        dice: OwnedDiceArtifacts,
    },
}

impl VmSecret {
    pub fn new(dice_artifacts: OwnedDiceArtifacts) -> Result<VmSecret> {
        if is_rp_secrets_supported() {
            // TODO(291213394): Change this to real dice protected secrets from Secretkeeeper.
            let fake_rp_secret = ZVec::new(VM_SECRET_SIZE)?;
            return Ok(Self::V2 { dice: dice_artifacts, rp_secret: fake_rp_secret });
        }
        Ok(Self::V1 { dice: dice_artifacts })
    }
    pub fn dice(&self) -> &OwnedDiceArtifacts {
        match self {
            Self::V2 { dice, .. } => dice,
            Self::V1 { dice } => dice,
        }
    }

    fn get_vm_secret(&self, salt: &[u8], identifier: &[u8], key: &mut [u8]) -> Result<()> {
        match self {
            Self::V2 { dice, rp_secret } => {
                hkdf(key, Md::sha256(), &rp_secret.concat(dice.cdi_seal())?, salt, identifier)?
            }
            Self::V1 { dice } => hkdf(key, Md::sha256(), dice.cdi_seal(), salt, identifier)?,
        }
        Ok(())
    }

    /// Derives a sealing key of `key_length` bytes from the VmSecret.
    /// Essentially key expansion.
    pub fn derive_sealing_key(&self, salt: &[u8], identifier: &[u8], key: &mut [u8]) -> Result<()> {
        self.get_vm_secret(salt, identifier, key)
    }
}

fn is_rp_secrets_supported() -> bool {
    // TODO(b/292209416): This value should be extracted from device tree.
    true
}
