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

use core::cmp;
use core::fmt;
use core::mem::size_of;
use core::num::NonZeroUsize;

use crate::hvc;

pub enum Error {
    /// Error during SMCCC TRNG call.
    Trng(hvc::trng::Error),
    /// Unsupported SMCCC TRNG version.
    UnsupportedSmcccTrngVersion((u16, u16)),
}

impl From<hvc::trng::Error> for Error {
    fn from(e: hvc::trng::Error) -> Self {
        Self::Trng(e)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Trng(e) => write!(f, "SMCCC TRNG error: {e}"),
            Self::UnsupportedSmcccTrngVersion((x, y)) => {
                write!(f, "Unsupported SMCCC TRNG version v{x}.{y}")
            }
        }
    }
}

pub struct Entropy;

impl Entropy {
    fn _init(&self) -> Result<()> {
        match hvc::trng_version()? {
            (1, _) => Ok(()),
            version => Err(Error::UnsupportedSmcccTrngVersion(version)),
        }
    }

    fn _fill_partial(&self, buffer: &mut [u8]) -> Result<Option<NonZeroUsize>> {
        const MAX_BYTES_PER_CALL: usize = size_of::<hvc::TrngRng64Entropy>();

        let chunk_len = cmp::min(buffer.len(), MAX_BYTES_PER_CALL);
        let chunk = &mut buffer[..chunk_len];

        if !chunk.is_empty() {
            let mut entropy = [0; MAX_BYTES_PER_CALL];
            let bits = usize::try_from(u8::BITS).unwrap();
            let (r0, r1, r2) = match hvc::trng_rnd64((chunk.len() * bits).try_into().unwrap()) {
                Err(hvc::trng::Error::NoEntropy) => return Ok(None),
                result => result?,
            };

            // SMCCC TRNG fills up registers with entropy starting from the "last" one.
            let mut words = entropy.chunks_exact_mut(size_of::<u64>());
            words.next().unwrap().clone_from_slice(&r2.to_ne_bytes());
            words.next().unwrap().clone_from_slice(&r1.to_ne_bytes());
            words.next().unwrap().clone_from_slice(&r0.to_ne_bytes());

            chunk.clone_from_slice(&entropy[..chunk.len()]);
        }

        Ok(NonZeroUsize::new(chunk.len()))
    }
}

impl super::Entropy for Entropy {
    fn init(&self) -> super::Result<()> {
        Ok(self._init()?)
    }

    fn fill_partial(&self, buffer: &mut [u8]) -> super::Result<Option<NonZeroUsize>> {
        Ok(self._fill_partial(buffer)?)
    }
}
