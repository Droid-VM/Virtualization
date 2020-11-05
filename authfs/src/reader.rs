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

//! A module for reading data by chunks.

use std::fs::File;
use std::io::{Error, ErrorKind, Result};
use std::os::unix::fs::FileExt;
use std::path::Path;

/// A trait for reading data by chunks. The data is assumed readonly and has fixed length. Chunks
/// can be read by specifying the chunk index. Only the last chunk may have incomplete chunk size.
pub trait ReadOnlyDataByChunk {
    /// Default chunk size.
    const CHUNK_SIZE: u64 = 4096;

    /// Returns the total length of data.
    fn len(&self) -> u64;

    /// Read the `chunk_index`-th chunk to `buf`. Each slice/chunk has size `CHUNK_SIZE` except for
    /// the last one, which can be an incomplete chunk.
    fn read_chunk(&self, chunk_index: u64, buf: &mut [u8]) -> Result<usize>;

    /// Returns the total number of available chunks.
    fn total_chunk_number(&self) -> u64 {
        (self.len() + Self::CHUNK_SIZE - 1) / Self::CHUNK_SIZE
    }

    /// Converts a chunk index to the range of byte offset.
    fn chunk_index_to_range(&self, size: u64, chunk_index: u64) -> Result<(u64, u64)> {
        let start = chunk_index * Self::CHUNK_SIZE;
        if start >= size {
            return Err(Error::new(ErrorKind::InvalidInput, "start >= size"));
        }
        let end = std::cmp::min(size, start + Self::CHUNK_SIZE);
        Ok((start, end))
    }
}

/// A read-only file that can be read by chunks.
pub struct ChunkedFileReader {
    file: File,
    size: u64,
}

impl ChunkedFileReader {
    /// Creates a `ChunkedFileReader` to read from for the specified `path`.
    #[allow(dead_code)]
    pub fn new(path: &Path) -> Result<ChunkedFileReader> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        Ok(ChunkedFileReader { file, size })
    }
}

impl ReadOnlyDataByChunk for ChunkedFileReader {
    fn len(&self) -> u64 {
        self.size
    }

    fn read_chunk(&self, chunk_index: u64, buf: &mut [u8]) -> Result<usize> {
        let (start, end) = self.chunk_index_to_range(self.len(), chunk_index)?;
        debug_assert!(end - start <= buf.len() as u64);
        self.file.read_at(buf, start)
    }
}

/// A read-only memory that can be read by chunks.
pub struct ChunkedDataReader {
    data: Vec<u8>,
}

impl ChunkedDataReader {
    /// Creates a `ChunkedDataReader` by taking over a `Vec<u8>` to be read by chunks.
    #[allow(dead_code)]
    pub fn new(data: Vec<u8>) -> Result<ChunkedDataReader> {
        Ok(ChunkedDataReader { data })
    }
}

impl ReadOnlyDataByChunk for ChunkedDataReader {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn read_chunk(&self, chunk_index: u64, buf: &mut [u8]) -> Result<usize> {
        let (start_u64, end_u64) = self.chunk_index_to_range(self.len(), chunk_index)?;
        let start = start_u64 as usize;
        let end = end_u64 as usize;
        let size = end - start;
        buf[..size].copy_from_slice(&self.data[start..end]);
        Ok(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn do_read_test<FH: ReadOnlyDataByChunk>(handle: FH) -> Result<()> {
        let mut buf = [0u8; 4096];
        assert!(handle.read_chunk(0, &mut buf).is_ok());
        assert!(handle.len() > 0);
        let last_index = (handle.len() + 4095) / 4096 - 1;
        assert!(handle.read_chunk(last_index, &mut buf).is_ok());
        assert!(handle.read_chunk(last_index + 1, &mut buf).is_err());
        Ok(())
    }

    /*
    #[test]
    #[ignore]
    fn test_read_local_file() -> Result<()> {
        let project_root = env!("CARGO_MANIFEST_DIR");
        let mut file_path = std::path::PathBuf::from(project_root);
        file_path.push("testdata/test.apk");

        do_read_test(ChunkedFileReader::new(&file_path)?)
    }
    */

    #[test]
    fn test_read_local_empty_file() -> Result<()> {
        let mut buf = [0u8; 4096];
        let reader = ChunkedDataReader::new(include_bytes!("../testdata/empty.file").to_vec())?;
        assert!(reader.read_chunk(0, &mut buf).is_err());
        Ok(())
    }

    #[test]
    fn test_read_in_memory_data() -> Result<()> {
        do_read_test(ChunkedDataReader::new(include_bytes!("../testdata/test.apk").to_vec())?)
    }

    #[test]
    fn test_read_in_memory_empty_data() -> Result<()> {
        let mut buf = [0u8; 4096];
        let reader = ChunkedDataReader::new(Vec::new())?;
        assert!(reader.read_chunk(0, &mut buf).is_err());
        Ok(())
    }
}
