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
use core::num::NonZeroUsize;

use crate::read_sysreg;
use crate::try_read_sysreg;

pub struct Entropy;

pub enum Error {
    /// ARMv8.5 FEAT_RND not implemented.
    Unimplemented,
}

pub type Result<T> = core::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Unimplemented => write!(f, "CPU doesn't implement FEAT_RND"),
        }
    }
}

impl Entropy {
    fn _init(&self) -> Result<()> {
        const ID_REG_FIELD_MASK: usize = 0xF;
        const ID_AA64ISAR0_EL1_RNDR_SHIFT: u32 = 60;
        let id_aa64isar0_el1 = read_sysreg!("id_aa64isar0_el1");

        if ((id_aa64isar0_el1 >> ID_AA64ISAR0_EL1_RNDR_SHIFT) & ID_REG_FIELD_MASK) == 0 {
            Err(Error::Unimplemented)
        } else {
            Ok(())
        }
    }

    fn _fill_partial(&self, s: &mut [u8]) -> Result<Option<NonZeroUsize>> {
        if s.is_empty() {
            return Ok(None);
        }

        // RNDRRS = (0b11, 0b011, 0b0010, 0b0100, 0b001) is not known by the toolchain.
        let size = if let Some(entropy) = try_read_sysreg!("s3_3_c2_c4_1") {
            let entropy = entropy.to_ne_bytes();
            let chunk_len = cmp::min(s.len(), entropy.len());
            let chunk = &mut s[..chunk_len];
            chunk.clone_from_slice(&entropy[..chunk_len]);
            chunk_len
        } else {
            0
        };

        Ok(NonZeroUsize::new(size))
    }
}

impl super::Entropy for Entropy {
    fn init(&self) -> super::Result<()> {
        Ok(self._init()?)
    }

    fn fill_partial(&self, s: &mut [u8]) -> super::Result<Option<NonZeroUsize>> {
        Ok(self._fill_partial(s)?)
    }
}
