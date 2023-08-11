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

//! Class for encapsulating & managing represent VM secrets.

use anyhow::{Context, ensure, Result};
use android_system_virtualmachineservice::aidl::android::system::virtualmachineservice::IVirtualMachineService::IVirtualMachineService;
use binder::Strong;
use diced_open_dice::{DiceArtifacts, OwnedDiceArtifacts};
use keystore2_crypto::ZVec;
use openssl::hkdf::hkdf;
use openssl::md::Md;
use rand::Fill;

// Size of the secret stored in Secretkeeper.
const SK_SECRET_SIZE: usize = 64;

pub enum VmSecret {
    // V2 secrets are derived from 2 independently secured secrets:
    //      1. Secretkeeper protected secrets (skp secret).
    //      2. Dice Sealing CDIs (Similar to V1).
    //
    // These are protected against rollback of boot images i.e. VM instance rebooted
    // with downgraded images will not have access to VM's secret.
    // V2 secrets require hardware support - Secretkeeper HAL, which (among other things)
    // is backed by tamper-evident storage, providing rollback protection to these secrets.
    V2 { dice: OwnedDiceArtifacts, skp_secret: ZVec },
    // V1 secrets are not protected against rollback of boot images.
    // They are reliable only if Anti-rollback protection was prevented by verified boot
    // ie, each stage (including pvmfw/Microdroid/Microdroid Manager) prevented downgrade of next stage.
    // These are now legacy secrets & used only when Secretkeeper HAL is not supported by device.
    V1 { dice: OwnedDiceArtifacts },
}

impl VmSecret {
    pub fn new(
        dice_artifacts: OwnedDiceArtifacts,
        vm_service: &Strong<dyn IVirtualMachineService>,
    ) -> Result<VmSecret> {
        if is_sk_supported() {
            ensure!(dice_artifacts.bcc().is_some(), "Dice chain missing");
            let mut skp_secret;

            // TODO(b/291213394): This should contains policy/dice chain for client authentication.
            let auth_det = Vec::new();
            let sk_service =
                vm_service.getSecretkeeper().context("Cannot connect to Secretkeeper").map_err(
                    |e| super::MicrodroidError::FailedToConnectToSecretkeeper(e.to_string()),
                )?;
            if super::is_new_instance() {
                skp_secret = Vec::with_capacity(SK_SECRET_SIZE);
                skp_secret.try_fill(&mut rand::thread_rng())?;
                sk_service.store(&skp_secret)?;
            } else {
                skp_secret = sk_service.read(&auth_det)?;
            }
            return Ok(Self::V2 { dice: dice_artifacts, skp_secret: ZVec::try_from(skp_secret)? });
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
            Self::V2 { dice, skp_secret } => {
                hkdf(key, Md::sha256(), &skp_secret.concat(dice.cdi_seal())?, salt, identifier)?
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

// Does the hardware support Secretkeeper.
fn is_sk_supported() -> bool {
    // TODO(b/292209416): This value should be extracted from device tree.
    true
}
