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

//! Utilities for zip handling

use anyhow::{bail, Result};
use std::io;
use std::io::Read;
use zip::spec::CentralDirectoryEnd;

const EOCD_MIN_SIZE: u32 = 22;
const ZIP64_DISK_NUMBER_MARKER: u16 = 0xffff;

#[derive(Debug, PartialEq)]
pub struct ZipSections {
    pub central_directory_offset: u32,
    pub central_directory_size: u32,
    pub eocd_offset: u32,
    pub eocd_size: u32,
}

/// Discover the layout of a zip file.
pub fn zip_sections<T: Read + io::Seek>(reader: &mut T) -> Result<ZipSections> {
    let (eocd, eocd_offset) = CentralDirectoryEnd::find_and_parse(reader)?;
    if eocd.disk_number == ZIP64_DISK_NUMBER_MARKER {
        bail!("Support for ZIP64 file is not implemented");
    }
    if eocd.disk_number != eocd.disk_with_central_directory {
        bail!("Support for multi-disk files is not implemented");
    }
    Ok(ZipSections {
        central_directory_offset: eocd.central_directory_offset,
        central_directory_size: eocd.central_directory_size,
        eocd_offset: eocd_offset as u32,
        eocd_size: EOCD_MIN_SIZE + eocd.zip_file_comment.len() as u32,
    })
}
