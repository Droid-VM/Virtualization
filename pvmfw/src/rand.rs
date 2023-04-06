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

pub use smccc_trng::Error;
pub use smccc_trng::Result;

/// Configure the source of entropy.
pub fn init() -> Result<()> {
    smccc_trng::init()
}

fn fill_with_entropy(buffer: &mut [u8]) -> Result<()> {
    let mut written = 0;
    while written < buffer.len() {
        if let Some(chunk_size) = smccc_trng::fill_partial(&mut buffer[written..])? {
            written += chunk_size.get();
        }
    }

    Ok(())
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
