/*
 * Copyright (C) 2020 The Android Open Source Project
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
use thiserror::Error;

use crate::crypto::{CryptoError, Sha256Hasher};
use crate::reader::ReadOnlyDataByChunk;

const ZEROS: [u8; 4096] = [0u8; 4096];

#[derive(Error, Debug)]
pub enum FsverityError {
    #[error("Cannot verify a block")]
    CannotVerify,
    #[error("I/O error")]
    Io(#[from] io::Error),
    #[error("Crypto")]
    UnexpectedCryptoError(#[from] CryptoError),
}

fn divide_roundup(dividend: u64, divisor: u64) -> u64 {
    (dividend + divisor - 1) / divisor
}

/// Returns an array of summed area table of level size in the fs-verity verity tree.
fn generate_offsets(mut data_size: u64, page_size: u64, hash_size: u64) -> Vec<u64> {
    let mut sizes = Vec::new();

    // Calculate offsets of all levels.
    loop {
        data_size = divide_roundup(data_size, page_size) * hash_size;
        let level_size = divide_roundup(data_size, page_size) * page_size;
        sizes.push(level_size);
        if data_size <= page_size {
            break;
        }
    }

    // Calculate accumulated offsets of all levels.
    let mut summed_area_table = Vec::with_capacity(sizes.len() + 1);
    summed_area_table.push(0);
    for size in sizes.iter().rev() {
        summed_area_table.push(summed_area_table.last().unwrap() + size);
    }
    summed_area_table.pop();
    summed_area_table
}

#[allow(dead_code)]
fn verity_check<T: ReadOnlyDataByChunk>(
    chunk: &[u8],
    chunk_index: u64,
    file_size: u64,
    merkle_tree: &T,
) -> Result<(), FsverityError> {
    let mut current_chunk = chunk;
    let mut current_chunk_index = chunk_index;
    let offsets = generate_offsets(file_size, T::CHUNK_SIZE, Sha256Hasher::HASH_SIZE as u64);

    let mut merkle_chunk = [0u8; 4096];
    for level_offset in offsets.iter().rev() {
        let padding_size = T::CHUNK_SIZE as usize - current_chunk.len();
        let hash = Sha256Hasher::new()?
            .update(&current_chunk)?
            .update(&ZEROS[..padding_size])?
            .finalize()?;

        let hash_offset_at_level = current_chunk_index * Sha256Hasher::HASH_SIZE as u64;
        let hash_offset_in_chunk = (hash_offset_at_level % T::CHUNK_SIZE) as usize;
        let size = merkle_tree
            .read_chunk((level_offset + hash_offset_at_level) / T::CHUNK_SIZE, &mut merkle_chunk)?;
        debug_assert_eq!(
            size,
            T::CHUNK_SIZE as usize,
            "size {} is not PAGE_SIZE({})",
            size,
            T::CHUNK_SIZE as usize
        );
        if hash
            != merkle_chunk[hash_offset_in_chunk..hash_offset_in_chunk + Sha256Hasher::HASH_SIZE]
        {
            return Err(FsverityError::CannotVerify);
        }

        current_chunk = &merkle_chunk[..];
        current_chunk_index = hash_offset_at_level / T::CHUNK_SIZE;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::ReadOnlyDataByChunk;
    use anyhow::Result;

    #[test]
    fn fsverity_verify_full_read_4k() -> Result<()> {
        let file = &include_bytes!("../testdata/input.4k")[..];
        let merkle_tree = &include_bytes!("../testdata/input.4k.merkle_dump")[..];

        let mut buf = [0u8; 4096];
        for i in 0..file.total_chunk_number() {
            let size = file.read_chunk(i, &mut buf[..])?;
            assert!(verity_check(&buf[..size], i, file.size(), &merkle_tree).is_ok());
        }
        Ok(())
    }

    #[test]
    fn fsverity_verify_full_read_4k1() -> Result<()> {
        let file = &include_bytes!("../testdata/input.4k1")[..];
        let merkle_tree = &include_bytes!("../testdata/input.4k1.merkle_dump")[..];

        let mut buf = [0u8; 4096];
        for i in 0..file.total_chunk_number() {
            let size = file.read_chunk(i, &mut buf[..])?;
            assert!(verity_check(&buf[..size], i, file.size(), &merkle_tree).is_ok());
        }
        Ok(())
    }

    #[test]
    fn fsverity_verify_full_read_4m() -> Result<()> {
        let file = &include_bytes!("../testdata/input.4m")[..];
        let merkle_tree = &include_bytes!("../testdata/input.4m.merkle_dump")[..];

        let mut buf = [0u8; 4096];
        for i in 0..file.total_chunk_number() {
            let size = file.read_chunk(i, &mut buf[..])?;
            assert!(verity_check(&buf[..size], i, file.size(), &merkle_tree).is_ok());
        }
        Ok(())
    }

    #[test]
    fn fsverity_verify_bad_merkle_tree() -> Result<()> {
        let file = &include_bytes!("../testdata/input.4m")[..];
        // First leaf node is corrupted.
        let merkle_tree = &include_bytes!("../testdata/input.4m.merkle_dump.bad")[..];

        // A lowest broken node (a 4K chunk that contains 128 sha256 hashes) will fail the read
        // failure of the underlying chunks, but not before or after.
        let mut buf = [0u8; 4096];
        let num_hashes = 4096 / 32;
        let last_index = num_hashes;
        for i in 0..last_index {
            let size = file.read_chunk(i, &mut buf[..])?;
            assert!(verity_check(&buf[..size], i, file.size(), &merkle_tree).is_err());
        }
        let size = file.read_chunk(last_index, &mut buf[..])?;
        assert!(verity_check(&buf[..size], last_index, file.size(), &merkle_tree).is_ok());
        Ok(())
    }
}
