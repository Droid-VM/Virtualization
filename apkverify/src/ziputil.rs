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
use byteorder::{LittleEndian, ReadBytesExt};
use std::io;
use std::io::Read;

const EOCD_SIGNATURE: u32 = 0x06054b50;
const EOCD_MIN_SIZE: u64 = 22;
const EOCD_COMMENT_MAX_SIZE: u16 = ::std::u16::MAX;
const EOCD_MAX_SIZE: u64 = EOCD_MIN_SIZE + EOCD_COMMENT_MAX_SIZE as u64;

/// EOCD is the center piece to find the layout of a zip file
struct EndOfCentralDirectory {
    central_directory_size: u32,
    central_directory_offset: u32,
    zip_file_comment_length: u16,
}

#[derive(Debug, PartialEq)]
pub struct ZipSections {
    pub central_directory_offset: u32,
    pub central_directory_size: u32,
    pub eocd_offset: u32,
    pub eocd_size: u32,
}

impl EndOfCentralDirectory {
    fn parse<T: Read>(reader: &mut T) -> Result<EndOfCentralDirectory> {
        let disk_number = reader.read_u16::<LittleEndian>()?;
        let disk_with_central_directory = reader.read_u16::<LittleEndian>()?;
        let _number_of_files_on_this_disk = reader.read_u16::<LittleEndian>()?;
        let _number_of_files = reader.read_u16::<LittleEndian>()?;
        let central_directory_size = reader.read_u32::<LittleEndian>()?;
        let central_directory_offset = reader.read_u32::<LittleEndian>()?;
        let zip_file_comment_length = reader.read_u16::<LittleEndian>()?;
        let mut zip_file_comment = vec![0; zip_file_comment_length as usize];
        reader.read_exact(&mut zip_file_comment)?;

        if disk_number == 0xffff {
            bail!("Support for ZIP64 file is not implemented");
        }
        if disk_number != disk_with_central_directory {
            bail!("Support for multi-disk files is not implemented");
        }

        Ok(EndOfCentralDirectory {
            central_directory_size,
            central_directory_offset,
            zip_file_comment_length,
        })
    }
}

/// Discover the layout of a zip file.
pub fn zip_sections<T: Read + io::Seek>(reader: &mut T) -> Result<ZipSections> {
    let file_length = reader.seek(io::SeekFrom::End(0))?;
    if file_length < EOCD_MIN_SIZE {
        bail!("Invalid zip header");
    }

    let search_upper_bound = file_length.saturating_sub(EOCD_MAX_SIZE);
    let mut pos = file_length - EOCD_MIN_SIZE;
    while pos >= search_upper_bound {
        reader.seek(io::SeekFrom::Start(pos as u64))?;
        if reader.read_u32::<LittleEndian>()? == EOCD_SIGNATURE {
            let eocd = EndOfCentralDirectory::parse(reader)?;
            return Ok(ZipSections {
                central_directory_offset: eocd.central_directory_offset,
                central_directory_size: eocd.central_directory_size,
                eocd_offset: pos as u32,
                eocd_size: EOCD_MIN_SIZE as u32 + eocd.zip_file_comment_length as u32,
            });
        }
        pos = match pos.checked_sub(1) {
            Some(p) => p,
            None => break,
        };
    }
    bail!("Could not find central directory end")
}

#[cfg(test)]
mod test {
    use super::*;
    use std::fs::File;

    #[test]
    fn test_zip_sections() -> Result<()> {
        let mut f = File::open("tests/data/test.apex")?;
        let section = zip_sections(&mut f)?;
        assert_eq!(
            section,
            ZipSections {
                central_directory_offset: 9224192,
                central_directory_size: 8795,
                eocd_offset: 9232987,
                eocd_size: 3493,
            }
        );
        Ok(())
    }
}
