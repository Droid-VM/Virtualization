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

//! Rust bindgen interface for FSVerity Metadata file (.fsv_meta)
use authfs_fsverity_metadata_bindgen::{
    fsverity_metadata_header, FSVERITY_SIGNATURE_TYPE_NONE, FSVERITY_SIGNATURE_TYPE_PKCS7,
    FSVERITY_SIGNATURE_TYPE_RAW,
};

/// Structure for parsed metadata.
#[allow(dead_code)]
pub struct FSVerityMetadata {
    /// Header for the metadata.
    pub header: fsverity_metadata_header,

    /// Optional signature for the metadata.
    pub signature: Option<Vec<u8>>,

    /// Index of the merkle tree blocks in the metadata file.
    /// Byte offset will be merkle_tree_index * CHUNK_SIZE.
    pub merkle_tree_index: u64,
}

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::slice::from_raw_parts_mut;

/// Common block and page size in Linux.
pub const CHUNK_SIZE: u64 = authfs_fsverity_metadata_bindgen::CHUNK_SIZE;

/// Derive a path of metadata for a given path.
/// e.g. "system/framework/foo.jar" -> "system/framework/foo.jar.fsv_meta"
pub fn get_fsverity_metadata_path(path: &Path) -> PathBuf {
    let mut os_string: OsString = path.into();
    os_string.push(".fsv_meta");
    os_string.into()
}

/// Parse metadata from given file, and returns a structure for the metadata.
pub fn parse_fsverity_metadata(file: &File) -> io::Result<FSVerityMetadata> {
    let header_size = size_of::<fsverity_metadata_header>();

    // SAFETY: no one other can mutate the raw buffer
    let header: fsverity_metadata_header = unsafe {
        let mut header: fsverity_metadata_header = zeroed();
        let buffer = from_raw_parts_mut(
            &mut header as *mut fsverity_metadata_header as *mut u8,
            header_size,
        );
        file.read_exact_at(buffer, 0)?;
        header
    };

    if header.version != 1 {
        return Err(io::Error::new(io::ErrorKind::Other, "unsupported metadata version"));
    }

    let signature = match header.signature_type {
        FSVERITY_SIGNATURE_TYPE_NONE => None,
        FSVERITY_SIGNATURE_TYPE_PKCS7 | FSVERITY_SIGNATURE_TYPE_RAW => {
            // TODO: unpad pkcs7?
            let mut buf = vec![0u8; header.signature_size as usize];
            file.read_exact_at(&mut buf, header_size as u64)?;
            Some(buf)
        }
        _ => return Err(io::Error::new(io::ErrorKind::Other, "unknown signature type")),
    };

    let merkle_tree_index =
        (header_size as u64 + header.signature_size as u64 + CHUNK_SIZE - 1) / CHUNK_SIZE;

    Ok(FSVerityMetadata { header, signature, merkle_tree_index })
}
