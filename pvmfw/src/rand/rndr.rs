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
use core::num::NonZeroUsize;

use crate::read_sysreg;

use super::Entropy;
use super::Error;
use super::Result;

pub(crate) struct RndrEntropy {}

pub(crate) static ENTROPY: RndrEntropy = RndrEntropy {};

macro_rules! read_rnd_reg {
    ($rndr:literal) => {{
        let mut r: usize;
        let mut failed: usize;
        // Safe because it reads a system register and we don't pass options(preserves_flags).
        unsafe {
            core::arch::asm!(
                concat!("mrs {}, ", $rndr),
                // Reading RNDR(RS) sets PSTATE.NZCV to 0b0000 on success, 0b0100 otherwise.
                 "cset {}, eq",
                 out(reg) r,
                 out(reg) failed,
                 options(nomem, nostack),
            )
        }
        if failed == 0 { Some(r) } else { None }
    }};
}

impl Entropy for RndrEntropy {
    fn init(&self) -> Result<()> {
        const ID_AA64ISAR0_EL1_RNDR_SHIFT: u32 = 60;
        let id_aa64isar0_el1 = read_sysreg!("id_aa64isar0_el1");

        if ((id_aa64isar0_el1 >> ID_AA64ISAR0_EL1_RNDR_SHIFT) & 0xF) == 0 {
            Err(Error::RndrUnavailable)
        } else {
            Ok(())
        }
    }

    fn fill_partial(&self, s: &mut [u8]) -> Result<Option<NonZeroUsize>> {
        if s.is_empty() {
            return Ok(None);
        }

        // RNDRRS = (0b11, 0b011, 0b0010, 0b0100, 0b001) is not known by the toolchain.
        let size = if let Some(entropy) = read_rnd_reg!("s3_3_c2_c8_1") {
            let entropy = entropy.to_ne_bytes();
            let chunk_len = cmp::min(s.len(), entropy.len());
            let chunk = &mut s[..chunk_len];
            chunk.clone_from_slice(&entropy[..chunk.len()]);
            chunk.len()
        } else {
            0
        };

        Ok(NonZeroUsize::new(size))
    }
}
