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

use anyhow::{anyhow, Result};

use crate::crypto::Sha256Hasher;
use crate::reader::ReadOnlyDataByChunk;

const PAGE_SIZE: usize = 4096;
const PAGE_SIZE_U64: u64 = PAGE_SIZE as u64;

// Assuming SHA-256 for now.
const HASH_SIZE: usize = 32;
const HASH_SIZE_U64: u64 = HASH_SIZE as u64;

fn divide_roundup(dividend: u64, divisor: u64) -> u64 {
    (dividend + divisor - 1) / divisor
}

/// Returns an array of summed area table of level size in the fs-verity verity tree.
fn generate_offsets(mut data_size: u64) -> Vec<u64> {
    let mut sizes = Vec::new();

    // Calculate offsets of all levels.
    while data_size > PAGE_SIZE_U64 {
        data_size = divide_roundup(data_size, PAGE_SIZE_U64) * HASH_SIZE_U64;
        let level_size = divide_roundup(data_size, PAGE_SIZE_U64) * PAGE_SIZE_U64;
        sizes.push(level_size);
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
) -> Result<()> {
    let mut current_chunk = chunk;
    let mut current_chunk_index = chunk_index;
    let offsets = generate_offsets(file_size);

    let mut merkle_chunk = [0u8; 4096];
    for level_offset in offsets.iter().rev() {
        let hash = Sha256Hasher::new()?
            .update(&current_chunk)?
            .update(&vec![0; PAGE_SIZE - current_chunk.len()])?
            .finalize()?;

        let hash_offset_at_level = current_chunk_index * HASH_SIZE_U64;
        let hash_offset_in_chunk = (hash_offset_at_level % PAGE_SIZE_U64) as usize;
        let size = merkle_tree
            .read_chunk((level_offset + hash_offset_at_level) / PAGE_SIZE_U64, &mut merkle_chunk)?;
        debug_assert_eq!(size, PAGE_SIZE, "size {} is not PAGE_SIZE({})", size, PAGE_SIZE);
        if hash != &merkle_chunk[hash_offset_in_chunk..hash_offset_in_chunk + HASH_SIZE] {
            return Err(anyhow!("fs-verity verification failed"));
        }

        current_chunk = &merkle_chunk[..];
        current_chunk_index = hash_offset_at_level / PAGE_SIZE_U64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::{ChunkedDataReader, ReadOnlyDataByChunk};
    use anyhow::Result;

    #[test]
    fn fsverity_verify_full_read() -> Result<()> {
        let file = ChunkedDataReader::new(include_bytes!("../testdata/test.apk").to_vec())?;
        let merkle_tree =
            ChunkedDataReader::new(include_bytes!("../testdata/test.apk.merkle_dump").to_vec())?;

        let mut buf = [0u8; 4096];
        for i in 0..file.total_chunk_number() {
            let size = file.read_chunk(i, &mut buf[..])?;
            assert!(super::verity_check(&buf[..size], i, file.len(), &merkle_tree).is_ok());
        }
        Ok(())
    }

    #[test]
    fn fsverity_verify_bad_merkle_tree() -> Result<()> {
        let file = ChunkedDataReader::new(include_bytes!("../testdata/test.apk").to_vec())?;
        // First leaf node is corrupted.
        let merkle_tree = ChunkedDataReader::new(
            include_bytes!("../testdata/test.apk.merkle_dump.bad").to_vec(),
        )?;

        // A lowest broken node (a 4K chunk that contains 128 sha256 hashes) will fail the read
        // failure of the underlying chunks, but not before or after.
        let mut buf = [0u8; 4096];
        let num_hashes = PAGE_SIZE_U64 / HASH_SIZE_U64;
        let last_index = num_hashes;
        for i in 0..last_index {
            let size = file.read_chunk(i, &mut buf[..])?;
            assert!(super::verity_check(&buf[..size], i, file.len(), &merkle_tree).is_err());
        }
        let size = file.read_chunk(last_index, &mut buf[..])?;
        assert!(super::verity_check(&buf[..size], last_index, file.len(), &merkle_tree).is_ok());
        Ok(())
    }
}
