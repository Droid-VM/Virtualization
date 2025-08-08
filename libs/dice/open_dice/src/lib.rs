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

//! Implements safe wrappers around the public API of libopen-dice for
//! both std and nostd usages.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(not(feature = "std"))]
extern crate core as std;

mod bcc;
mod dice;
mod error;
mod ops;
mod retry;

pub use bcc::{bcc_format_config_descriptor, bcc_handover_parse, BccHandover, DiceConfigValues};
#[cfg(feature = "multialg")]
pub use bcc::{bcc_handover_main_flow, bcc_main_flow};
#[cfg(feature = "multialg")]
pub use dice::{
    derive_cdi_certificate_id, derive_cdi_private_key_seed, dice_main_flow, DiceContext,
};
pub use dice::{
    Cdi, CdiValues, Config, DiceArtifacts, DiceMode, Hash, Hidden, InlineConfig, InputValues,
    KeyAlgorithm, PrivateKey, PrivateKeySeed, CDI_SIZE, HASH_SIZE, HIDDEN_SIZE, ID_SIZE,
    PRIVATE_KEY_SEED_SIZE, VM_KEY_ALGORITHM,
};
pub use error::{DiceError, Result};
#[cfg(feature = "multialg")]
pub use ops::{
    derive_cdi_leaf_priv, derive_cdi_leaf_priv_multialg, keypair_from_seed,
    keypair_from_seed_multialg, sign, sign_cose_sign1_multialg,
    sign_cose_sign1_with_cdi_leaf_priv_multialg, verify_multialg,
};
pub use ops::{generate_certificate, hash, kdf, verify};
pub use retry::{
    retry_bcc_format_config_descriptor, retry_generate_certificate, retry_sign_cose_sign1,
    OwnedDiceArtifacts,
};
#[cfg(feature = "multialg")]
pub use retry::{
    retry_bcc_main_flow, retry_dice_main_flow, retry_sign_cose_sign1_multialg,
    retry_sign_cose_sign1_with_cdi_leaf_priv, retry_sign_cose_sign1_with_cdi_leaf_priv_multialg,
};
