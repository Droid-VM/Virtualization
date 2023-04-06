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

mod smccc_trng;

use core::fmt;
use core::num::NonZeroUsize;

use log::info;

pub enum Error {
    /// Failed to initialize a valid source of entropy.
    NoEntropySource,
    /// SMCCC TRNG Error.
    SmcccTrng(smccc_trng::Error),
}

impl From<smccc_trng::Error> for Error {
    fn from(e: smccc_trng::Error) -> Self {
        Self::SmcccTrng(e)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NoEntropySource => write!(f, "Failed to initialize a valid source of entropy"),
            Self::SmcccTrng(e) => write!(f, "SMCCC TRNG error: {e}"),
        }
    }
}

trait Entropy {
    /// Discover and initialize the entropy source.
    fn init(&self) -> Result<()>;
    /// Obtain as much entropy as possible into a buffer.
    ///
    /// Reads one batch of entropy and writes it in the first bytes of the buffer. This function is
    /// intended to be called in a loop until the buffer is full and returns the number of bytes
    /// written or None when no entropy was available.
    fn fill_partial(&self, buffer: &mut [u8]) -> Result<Option<NonZeroUsize>>;
    /// Fill a buffer with entropy.
    fn fill(&self, buffer: &mut [u8]) -> Result<()> {
        let mut written = 0;
        while written < buffer.len() {
            if let Some(chunk_size) = self.fill_partial(&mut buffer[written..])? {
                written += chunk_size.get();
            }
        }

        Ok(())
    }
}

static mut ENTROPY: Option<&dyn Entropy> = None;

fn select_entropy() -> Result<&'static dyn Entropy> {
    let entropies = [&smccc_trng::Entropy];

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

fn fill_with_entropy(buffer: &mut [u8]) -> Result<()> {
    // SAFETY - Variable only set here and pvmfw is single threaded.
    let entropy = unsafe { ENTROPY.unwrap() };

    entropy.fill(buffer)
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
