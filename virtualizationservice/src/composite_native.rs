// Copyright 2021, The Android Open Source Project
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

use anyhow::Error;
use crc32fast::Hasher;
use std::convert::TryInto;
use std::fs::File;
use std::io::Write;
use uuid::Uuid;

const GPT_NUM_PARTITIONS: u32 = 128;
const SECTOR_SIZE: u64 = 1 << 9;
const MBR_PARTITION_ENTRY_SIZE: usize = 16;
const GPT_HEADER_SIZE: u32 = 92;
const GPT_BEGINNING_SIZE: u64 = SECTOR_SIZE * 40; // TODO: Assert that this matches something.
const PARTITION_ALIGNMENT_SIZE: usize = 3072; // TODO: Assert that this is correct.
const GPT_END_SIZE: u64 = SECTOR_SIZE * 33; // TODO: Assert that this matches something.
const GPT_PARTITION_ENTRY_SIZE: u32 = 128;
const HEADER_PADDING_LENGTH: usize = SECTOR_SIZE as usize - GPT_HEADER_SIZE as usize;
// Keep all partitions 4k aligned for performance.
const PARTITION_SIZE_SHIFT: u8 = 12;
// Keep the disk size a multiple of 64k for crosvm's virtio_blk driver.
const DISK_SIZE_SHIFT: u8 = 16;

const LINUX_FILESYSTEM_GUID: Uuid = Uuid::from_u128(0x0FC63DAF_8483_4772_8E79_3D69D8477DE4);
const EFI_SYSTEM_PARTITION_GUID: Uuid = Uuid::from_u128(0xC12A7328_F81F_11D2_BA4B_00A0C93EC93B);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartitionInfo {
    name: String,
    partition_type: ImagePartitionType,
    size: u64,
    offset: u64,
}

fn align_to_power_of_2(val: u64, align_log: u8) -> u64 {
    let align = 1 << align_log;
    ((val + (align - 1)) / align) * align
}

impl PartitionInfo {
    fn aligned_size(&self) -> u64 {
        align_to_power_of_2(self.size, PARTITION_SIZE_SHIFT)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ImagePartitionType {
    LinuxFilesystem,
    EfiSystemPartition,
}

impl ImagePartitionType {
    fn guid(self) -> Uuid {
        match self {
            LinuxFilesystem => LINUX_FILESYSTEM_GUID,
            EfiSystemPartition => EFI_SYSTEM_PARTITION_GUID,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct GptHeader {
    signature: [u8; 8],
    revision: [u8; 4],
    header_size: u32,
    header_crc32: u32,
    current_lba: u64,
    backup_lba: u64,
    first_usable_lba: u64,
    last_usable_lba: u64,
    disk_guid: Uuid,
    partition_entries_lba: u64,
    num_partition_entries: u32,
    partition_entry_size: u32,
    partition_entries_crc32: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GptPartitionEntry {
    partition_type_guid: Uuid,
    unique_partition_guid: Uuid,
    first_lba: u64,
    last_lba: u64,
    attributes: u64,
    /// UTF-16LE
    partition_name: [u16; 36],
}

// TODO: Derive this once arrays of more than 32 elements have default values.
impl Default for GptPartitionEntry {
    fn default() -> Self {
        Self {
            partition_type_guid: Default::default(),
            unique_partition_guid: Default::default(),
            first_lba: 0,
            last_lba: 0,
            attributes: 0,
            partition_name: [0; 36],
        }
    }
}

/*#[repr(C, packed)]
struct GptBeginning {
    protective_mbr: MasterBootRecord,
    header: GptHeader,
    header_padding: [u8; 420],
    entries: [GptPartitionEntry; GPT_NUM_PARTITIONS],
    partition_alignment: [u8; 3072],
}*/

fn write_protective_mbr(file: &mut impl Write, disk_size: u64) -> Result<(), Error> {
    // Bootstrap code
    file.write_all(&[0; 446])?;

    // Partition status
    file.write_all(&[0x00])?;
    // Begin CHS
    file.write_all(&[0; 3])?;
    // Partition type
    file.write_all(&[0xEE])?;
    // End CHS
    file.write_all(&[0; 3])?;
    let first_lba: u32 = 1;
    file.write_all(&first_lba.to_le_bytes())?;
    let number_of_sectors = disk_size / SECTOR_SIZE;
    file.write_all(&number_of_sectors.to_le_bytes())?;

    // Three more empty partitions
    file.write_all(&[0; MBR_PARTITION_ENTRY_SIZE * 3])?;

    // Boot signature
    file.write_all(&[0x55, 0xAA])?;

    Ok(())
}

fn uuid_generate() -> [u8; 16] {
    unimplemented!();
}

/// Write a UUID in the mixed-endian format which GPT uses for GUIDs.
fn write_guid(out: &mut impl Write, guid: Uuid) -> Result<(), Error> {
    let guid_fields = guid.as_fields();
    out.write_all(&guid_fields.0.to_le_bytes())?;
    out.write_all(&guid_fields.1.to_le_bytes())?;
    out.write_all(&guid_fields.2.to_le_bytes())?;
    out.write_all(guid_fields.3)?;

    Ok(())
}

impl GptHeader {
    fn write_bytes(&self, out: &mut impl Write) -> Result<(), Error> {
        out.write_all(&self.signature)?;
        out.write_all(&self.revision)?;
        out.write_all(&self.header_size.to_le_bytes())?;
        out.write_all(&self.header_crc32.to_le_bytes())?;
        // Reserved
        out.write_all(&[0; 4])?;
        out.write_all(&self.current_lba.to_le_bytes())?;
        out.write_all(&self.backup_lba.to_le_bytes())?;
        out.write_all(&self.first_usable_lba.to_le_bytes())?;
        out.write_all(&self.last_usable_lba.to_le_bytes())?;

        // GUID is mixed-endian for some reason, so we can't just use
        // `Uuid::as_bytes()`.
        write_guid(out, self.disk_guid);

        out.write_all(&self.partition_entries_lba.to_le_bytes())?;
        out.write_all(&self.num_partition_entries.to_le_bytes())?;
        out.write_all(&self.partition_entry_size.to_le_bytes())?;
        out.write_all(&self.partition_entries_crc32.to_le_bytes())?;
        Ok(())
    }
}

impl GptPartitionEntry {
    fn write_bytes(&self, out: &mut impl Write) -> Result<(), Error> {
        write_guid(out, self.partition_type_guid)?;
        write_guid(out, self.unique_partition_guid)?;
        out.write_all(&self.first_lba.to_le_bytes())?;
        out.write_all(&self.last_lba.to_le_bytes())?;
        out.write_all(&self.attributes.to_le_bytes())?;
        for code_unit in &self.partition_name {
            out.write_all(&code_unit.to_le_bytes())?;
        }
        Ok(())
    }
}

fn write_gpt_header(
    out: &mut impl Write,
    disk_guid: Uuid,
    partition_entries_crc32: u32,
    secondary_table_offset: u64,
    secondary: bool,
) -> Result<(), Error> {
    let primary_header_lba = 1;
    let secondary_header_lba = (secondary_table_offset + GPT_END_SIZE) / SECTOR_SIZE - 1;
    let mut gpt_header = GptHeader {
        signature: *b"EFI PART",
        revision: [0, 0, 1, 0],
        header_size: GPT_HEADER_SIZE,
        current_lba: if secondary { secondary_header_lba } else { 1 },
        backup_lba: if secondary { 1 } else { secondary_header_lba },
        first_usable_lba: GPT_BEGINNING_SIZE / SECTOR_SIZE,
        last_usable_lba: secondary_table_offset / SECTOR_SIZE - 1,
        disk_guid,
        partition_entries_lba: 2,
        num_partition_entries: GPT_NUM_PARTITIONS,
        partition_entry_size: GPT_PARTITION_ENTRY_SIZE,
        partition_entries_crc32,
        header_crc32: 0,
    };

    // Write once to a temporary buffer to calculate the CRC.
    let mut header_without_crc = [0u8; GPT_HEADER_SIZE as usize];
    let write: &mut [u8] = &mut header_without_crc; // TODO: This looks weird
    gpt_header.write_bytes(&mut write)?;
    let mut hasher = Hasher::new();
    hasher.update(&header_without_crc);
    gpt_header.header_crc32 = hasher.finalize();

    gpt_header.write_bytes(out)?;

    Ok(())
}

/// Write protective MBR and primary GPT table.
fn write_beginning(
    file: &mut File,
    disk_guid: Uuid,
    partitions: &[u8],
    partition_entries_crc32: u32,
    secondary_table_offset: u64,
) -> Result<(), Error> {
    let disk_size = align_to_power_of_2(secondary_table_offset + GPT_END_SIZE, DISK_SIZE_SHIFT);
    write_protective_mbr(file, disk_size)?;
    write_gpt_header(file, disk_guid, partition_entries_crc32, secondary_table_offset, false)?;
    file.write_all(&[0; HEADER_PADDING_LENGTH])?;
    // Write partition entries, including unused ones.
    file.write_all(partitions)?;
    // Write zeroes to align the first partition appropriately.
    file.write_all(&[0; PARTITION_ALIGNMENT_SIZE])?;

    Ok(())
}

/// Write secondary GPT table.
fn write_end(
    file: &mut File,
    disk_guid: Uuid,
    partitions: &[u8],
    partition_entries_crc32: u32,
    secondary_table_offset: u64,
) -> Result<(), Error> {
    // Write partition entries, including unused ones.
    file.write_all(partitions)?;
    write_gpt_header(file, disk_guid, partition_entries_crc32, secondary_table_offset, true)?;
    file.write_all(&[0; HEADER_PADDING_LENGTH])?;

    // Pad out to aligned disk size.
    let used_disk_size = secondary_table_offset + GPT_END_SIZE;
    let disk_size = align_to_power_of_2(used_disk_size, DISK_SIZE_SHIFT);
    let padding = disk_size - used_disk_size;
    file.write_all(&[0; padding])?;

    Ok(())
}

pub fn create_composite_disk(
    partitions: Vec<PartitionInfo>,
    header_file: &mut File,
    footer_file: &mut File,
    output_composite: &mut File,
) -> Result<(), Error> {
    // Write partitions to a temporary buffer to calculate the CRC.
    let mut partitions_buffer =
        [0u8; GPT_NUM_PARTITIONS as usize * GPT_PARTITION_ENTRY_SIZE as usize];
    let write: &mut [u8] = &mut partitions_buffer;
    let mut next_disk_offset = GPT_BEGINNING_SIZE;
    for partition in partitions {
        let mut partition_name: Vec<u16> = partition.name.encode_utf16().collect();
        partition_name.resize(36, 0);
        let offset = next_disk_offset;
        let aligned_size = partition.aligned_size();
        next_disk_offset += aligned_size;
        GptPartitionEntry {
            partition_type_guid: partition.partition_type.guid(),
            unique_partition_guid: Uuid::new_v4(),
            first_lba: offset / SECTOR_SIZE,
            last_lba: (offset + aligned_size) / SECTOR_SIZE - 1,
            attributes: 0,
            partition_name: partition_name.try_into().unwrap(),
        }
        .write_bytes(&mut write)?;
    }
    let secondary_table_offset = next_disk_offset;
    let mut hasher = Hasher::new();
    hasher.update(&partitions_buffer);
    let partition_entries_crc32 = hasher.finalize();

    let disk_guid = Uuid::new_v4();
    write_beginning(
        header_file,
        disk_guid,
        &partitions_buffer,
        partition_entries_crc32,
        secondary_table_offset,
    )?;
    write_end(
        footer_file,
        disk_guid,
        &partitions_buffer,
        partition_entries_crc32,
        secondary_table_offset,
    )?;
    let composite_proto = make_composite_disk_spec(header_file, footer_file);
    output_composite.write(composite_proto)?;

    Ok(())
}
