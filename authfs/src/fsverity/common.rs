/*
 * Copyright (C) 2021 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use std::io;
use std::mem;
use std::slice;

use thiserror::Error;

use super::sys::{fsverity_descriptor, FS_VERITY_HASH_ALG_SHA256};
use crate::common::{divide_roundup, COMMON_PAGE_SIZE};
use crate::crypto::{CryptoError, Sha256Hash, Sha256Hasher};

#[derive(Error, Debug)]
pub enum FsverityError {
    #[error("Cannot verify a signature")]
    BadSignature,
    #[error("Insufficient data, only got {0}")]
    InsufficientData(usize),
    #[error("Cannot verify a block")]
    CannotVerify,
    #[error("I/O error")]
    Io(#[from] io::Error),
    #[error("Crypto")]
    UnexpectedCryptoError(#[from] CryptoError),
    #[error("Invalid state")]
    InvalidState,
}

fn log128_ceil(num: u64) -> Option<u64> {
    match num {
        0 => None,
        n => Some(divide_roundup(64 - (n - 1).leading_zeros() as u64, 7)),
    }
}

/// Return the Merkle tree height for our tree configuration, or None if the size is 0.
pub fn merkle_tree_height(data_size: u64) -> Option<u64> {
    let hashes_per_node = COMMON_PAGE_SIZE / Sha256Hasher::HASH_SIZE as u64;
    let hash_pages = divide_roundup(data_size, hashes_per_node * COMMON_PAGE_SIZE);
    log128_ceil(hash_pages)
}

pub fn build_fsverity_digest(
    root_hash: &Sha256Hash,
    file_size: u64,
) -> Result<Sha256Hash, CryptoError> {
    // The latter 32 bytes are only used with SHA512.
    let mut root_hash_buffer = [0u8; 64];
    root_hash_buffer[..root_hash.len()].copy_from_slice(root_hash);

    let descriptor = fsverity_descriptor {
        version: 1u8,
        hash_algorithm: FS_VERITY_HASH_ALG_SHA256 as u8,
        log_blocksize: 12u8, // log_2(4096)
        salt_size: 0u8,
        __reserved_0x04: 0u32,
        data_size: file_size.to_le(),
        root_hash: root_hash_buffer,
        salt: [0u8; 32],
        __reserved: [0u8; 144],
    };

    let ptr = &descriptor as *const fsverity_descriptor as *const u8;
    let slice = unsafe {
        // SAFETY: the original struct outlives the coerced slice.
        slice::from_raw_parts(ptr, mem::size_of::<fsverity_descriptor>())
    };
    Sha256Hasher::new()?.update(slice)?.finalize()
}
