// Copyright 2024, The Android Open Source Project
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

//! `Ops` implementation to use libavb_rs APIs.

use avb::{IoError, IoResult, PublicKeyForPartitionInfo};
use std::{
    ffi::CStr,
    io::{Read, Seek, SeekFrom},
};

pub(crate) struct Ops<R: Read + Seek> {
    /// Image source.
    pub image: R,
    /// Offset into `image`.
    pub offset: u64,
    /// Size of `image` to use.
    pub size: u64,
    /// Extracted public key.
    pub public_key: Vec<u8>,
}

impl<R: Read + Seek> avb::Ops for Ops<R> {
    fn read_from_partition(
        &mut self,
        _partition: &CStr,
        offset: i64,
        buffer: &mut [u8],
    ) -> IoResult<usize> {
        // Negative `offset` means seek back from the end.
        let read_position = match offset >= 0 {
            true => self.offset + (offset as u64),
            false => self.offset + self.size - (offset.abs_diff(0)),
        };

        // We only have a single "partition" which is the provided image, so always read that.
        self.image.seek(SeekFrom::Start(read_position)).map_err(|_| IoError::Io)?;
        self.image.read_exact(buffer).map_err(|_| IoError::Io)?;
        Ok(buffer.len())
    }

    fn validate_vbmeta_public_key(
        &mut self,
        public_key: &[u8],
        _public_key_metadata: Option<&[u8]>,
    ) -> IoResult<bool> {
        // We don't validate the public key immediately, instead we save the public key here so we
        // can provide it to the user for validation later.
        self.public_key = public_key.into();
        Ok(true)
    }

    fn read_rollback_index(&mut self, _rollback_index_location: usize) -> IoResult<u64> {
        // This library does not check rollback indices, use 0 so that all images succeed.
        Ok(0)
    }

    fn write_rollback_index(
        &mut self,
        _rollback_index_location: usize,
        _index: u64,
    ) -> IoResult<()> {
        // Not used.
        Err(IoError::NotImplemented)
    }

    fn read_is_device_unlocked(&mut self) -> IoResult<bool> {
        // Not used but we have to return something here for libavb.
        Ok(false)
    }

    fn get_size_of_partition(&mut self, _partition: &CStr) -> IoResult<u64> {
        Ok(self.size)
    }

    fn read_persistent_value(&mut self, _name: &CStr, _value: &mut [u8]) -> IoResult<usize> {
        // Not used.
        Err(IoError::NotImplemented)
    }

    fn write_persistent_value(&mut self, _name: &CStr, _value: &[u8]) -> IoResult<()> {
        // Not used.
        Err(IoError::NotImplemented)
    }

    fn erase_persistent_value(&mut self, _name: &CStr) -> IoResult<()> {
        // Not used.
        Err(IoError::NotImplemented)
    }

    fn validate_public_key_for_partition(
        &mut self,
        _partition: &CStr,
        _public_key: &[u8],
        _public_key_metadata: Option<&[u8]>,
    ) -> IoResult<PublicKeyForPartitionInfo> {
        // Not used.
        Err(IoError::NotImplemented)
    }
}
