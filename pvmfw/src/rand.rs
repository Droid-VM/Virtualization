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

mod rndr;
mod smccc_trng;

use core::fmt;
use core::num::NonZeroUsize;

use log::info;

pub enum Error {
    /// Failed to initialize a valid source of entropy.
    NoEntropySource,
    /// SMCCC TRNG Error.
    SmcccTrng(smccc_trng::Error),
    /// ARMv8.5 FEAT_RND Error.
    Rndr(rndr::Error),
}

impl From<smccc_trng::Error> for Error {
    fn from(e: smccc_trng::Error) -> Self {
        Self::SmcccTrng(e)
    }
}

impl From<rndr::Error> for Error {
    fn from(e: rndr::Error) -> Self {
        Self::Rndr(e)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NoEntropySource => write!(f, "Failed to initialize a valid source of entropy"),
            Self::SmcccTrng(e) => write!(f, "SMCCC TRNG error: {e}"),
            Self::Rndr(e) => write!(f, "RNDR error: {e}"),
        }
    }
}

trait Entropy {
    fn init(&self) -> Result<()>;
    fn fill_partial(&self, s: &mut [u8]) -> Result<Option<NonZeroUsize>>;
    fn fill(&self, s: &mut [u8]) -> Result<()> {
        let mut written = 0;
        while written < s.len() {
            if let Some(chunk_size) = self.fill_partial(&mut s[written..])? {
                written += chunk_size.get();
            }
        }

        Ok(())
    }
}

static mut ENTROPY: Option<&dyn Entropy> = None;

fn select_entropy() -> Result<&'static dyn Entropy> {
    let entropies: [&dyn Entropy; 2] = [&rndr::Entropy, &smccc_trng::Entropy];

    for entropy in entropies {
        if let Err(e) = entropy.init() {
            info!("Failed to initialize entropy source: {e}")
        } else {
            return Ok(entropy);
        }
    }

    Err(Error::NoEntropySource)
}

/// Configure the source of entropy.
pub fn init() -> Result<()> {
    // SAFETY - Variable only set here and pvmfw is single threaded.
    unsafe { ENTROPY = Some(select_entropy()?) };

    Ok(())
}

fn fill_with_entropy(s: &mut [u8]) -> Result<()> {
    // SAFETY - Variable only set here and pvmfw is single threaded.
    let entropy = unsafe { ENTROPY.unwrap() };

    entropy.fill(s)
}

pub fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut arr = [0; N];
    fill_with_entropy(&mut arr)?;
    Ok(arr)
}

#[no_mangle]
extern "C" fn CRYPTO_sysrand_for_seed(out: *mut u8, req: usize) {
    CRYPTO_sysrand(out, req)
}

#[no_mangle]
extern "C" fn CRYPTO_sysrand(out: *mut u8, req: usize) {
    // SAFETY - We need to assume that out points to valid memory of size req.
    let s = unsafe { core::slice::from_raw_parts_mut(out, req) };
    let _ = fill_with_entropy(s);
}
