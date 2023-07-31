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

//! Android VM DTBO Extractor

use anyhow::{anyhow, Context, Result};
use rustutils::system_properties;
use std::fs::{create_dir, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::size_of;
use std::path::Path;

const DTBO_IMG_PATH_PREFIX: &str = "/dev/block/by-name/dtbo";
const VM_DTBO_DIR: &str = "/data/vm_dtbo/";

/// The structure of DT table header in dtbo.img.
/// https://source.android.com/docs/core/architecture/dto/partitions
#[derive(Debug)]
struct DtTableHeader {
    /// DT_TABLE_MAGIC
    _magic: u32,
    /// includes dt_table_header + all dt_table_entry and all dtb/dtbo
    _total_size: u32,
    /// sizeof(dt_table_header)
    _header_size: u32,
    /// sizeof(dt_table_entry)
    _dt_entry_size: u32,
    /// number of dt_table_entry
    _dt_entry_count: u32,
    /// offset to the first dt_table_entry from head of dt_table_header
    _dt_entries_offset: u32,
    /// flash page size we assume
    _page_size: u32,
    /// DTBO image version, the current version is 0. The version will be
    /// incremented when the dt_table_header struct is updated.
    _version: u32,
}

impl DtTableHeader {
    fn new(values: &[u32]) -> Result<DtTableHeader> {
        // TODO: Check DtTableHeader size
        Ok(DtTableHeader {
            _magic: values[0],
            _total_size: values[1],
            _header_size: values[2],
            _dt_entry_size: values[3],
            _dt_entry_count: values[4],
            _dt_entries_offset: values[5],
            _page_size: values[6],
            _version: values[7],
        })
    }
}

/// The structure of each DT table entry (v0) in dtbo.img.
/// https://source.android.com/docs/core/architecture/dto/partitions
#[derive(Debug)]
struct DtTableEntry {
    /// size of each DT
    _dt_size: u32,
    /// offset from head of dt_table_header
    _dt_offset: u32,
    /// optional, must be zero if unused
    _id: u32,
    /// optional, must be zero if unused
    _rev: u32,
    /// optional, must be zero if unused
    _custom: [u32; 4],
}

impl DtTableEntry {
    fn new(values: &[u32]) -> Result<DtTableEntry> {
        // TODO: Check DtTableEntry size
        let mut custom = [0; 4];
        custom.copy_from_slice(&values[4..]); // TODO: extend_from_slice
        let dt_table_entry = DtTableEntry {
            _dt_size: values[0],
            _dt_offset: values[1],
            _id: values[2],
            _rev: values[3],
            _custom: custom,
        };
        Ok(dt_table_entry)
    }
}

fn get_dtbo_img_path() -> Result<String> {
    let binding = system_properties::read("ro.boot.slot_suffix")?;
    let slot_suffix = binding.as_deref().ok_or_else(|| anyhow!("slot_suffix is none"))?;
    Ok(DTBO_IMG_PATH_PREFIX.to_string() + slot_suffix)
}

fn get_dt_table_header(file: &mut File) -> Result<DtTableHeader> {
    file.seek(SeekFrom::Start(0)).context("Cannot seek the offset of dt_table_header")?;

    let mut values = Vec::new();
    for _ in 0..8 {
        let mut buffer = [0; size_of::<u32>()];
        file.read_exact(&mut buffer).context("Failed to read dt_table_header")?;
        values.push(u32::from_be_bytes(buffer));
    }
    let dt_table_header = DtTableHeader::new(&values)?;

    println!("DT table header of dtbo.img: {:?}", dt_table_header);
    Ok(dt_table_header)
}

fn get_dt_table_entries(file: &mut File, header: &DtTableHeader) -> Result<Vec<DtTableEntry>> {
    file.seek(SeekFrom::Start(header._dt_entries_offset.into()))
        .context("Cannot seek the offset of dt_table_entry")?;

    let mut dt_table_entries = Vec::new();
    for _ in 0..header._dt_entry_count {
        let mut values = Vec::new();
        for _ in 0..8 {
            let mut buffer = [0; size_of::<u32>()];
            file.read_exact(&mut buffer).context("Failed to read dt_table_entry")?;
            values.push(u32::from_be_bytes(buffer));
        }
        let dt_table_entry = DtTableEntry::new(&values)?;
        dt_table_entries.push(dt_table_entry);
    }
    println!("DT table entries of dtbo.img: {:?}", dt_table_entries);
    Ok(dt_table_entries)
}

// TODO: filter vm dtbo files only
fn save_dtbo_file(dtbo_img_file: &mut File, entry: &DtTableEntry, save_path: &str) -> Result<()> {
    dtbo_img_file
        .seek(SeekFrom::Start(entry._dt_offset.into()))
        .context("Cannot seek the offset of device tree")?;

    let mut buffer = Vec::new();
    buffer.resize(entry._dt_size.try_into()?, 0);
    dtbo_img_file.read_exact(&mut buffer).context("Failed to read device tree")?;

    // TODO: check if file has right permission.
    let mut vm_dtbo_file = File::create(save_path).context("Failed to create vm dtbo file")?;
    vm_dtbo_file.write_all(&buffer).context("Failed to write dtbo file")?;
    Ok(())
}

fn main() -> Result<()> {
    println!("Running VM DTBO Extractor");

    let dtbo_path = get_dtbo_img_path()?;
    println!("Full path of dtbo.img: {:?}", dtbo_path);
    let mut dtbo_img = File::options()
        .read(true)
        .write(false)
        .open(dtbo_path)
        .context("Failed to open DTBO partition")?;

    let dt_table_header = get_dt_table_header(&mut dtbo_img)?;
    let dt_table_entries = get_dt_table_entries(&mut dtbo_img, &dt_table_header)?;

    let vm_dtbo_dir = Path::new(VM_DTBO_DIR);
    if !(vm_dtbo_dir.exists()) {
        // TODO: check if directory has right permission.
        create_dir(vm_dtbo_dir).context("Failed to create vm dtbo directory")?;
    }
    for (idx, dt_table_entry) in dt_table_entries.iter().enumerate() {
        let vm_dtbo_path = VM_DTBO_DIR.to_string() + &idx.to_string() + ".dtbo";
        save_dtbo_file(&mut dtbo_img, dt_table_entry, &vm_dtbo_path)?;
    }
    Ok(())
}
