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

//! Support for reading and writing to the instance.img.

use crate::crypto;
use crate::crypto::AeadCtx;
use crate::dice::PartialInputs;
use crate::gpt;
use crate::gpt::Partition;
use crate::gpt::Partitions;
use crate::virtio::pci::VirtIOBlkIterator;
use core::fmt;
use core::mem::size_of;
use core::num::NonZeroUsize;
use diced_open_dice::DiceMode;
use diced_open_dice::Hash;
use diced_open_dice::Hidden;
use uuid::Uuid;
use virtio_drivers::transport::pci::bus::PciRoot;

pub enum Error {
    /// Encountered an empty pvmfw instance.img entry.
    EmptyPvmfwEntry,
    /// Unexpected I/O error while accessing the underlying disk.
    FailedIo(gpt::Error),
    /// Failed to decrypt the entry.
    FailedOpen(crypto::ErrorIterator),
    /// Failed to encrypt the entry.
    FailedSeal(crypto::ErrorIterator),
    /// Impossible to create a new instance.img entry.
    InstanceImageFull,
    /// Badly formatted instance.img header block.
    InvalidInstanceImageHeader,
    /// No instance.img ("vm-instance") partition found.
    MissingInstanceImage,
    /// The instance.img doesn't contain a header.
    MissingInstanceImageHeader,
    /// Attempted to write to an existing entry.
    OverwritingEntry,
    /// Authority hash found in the pvmfw instance.img entry doesn't match the trusted public key.
    RecordedAuthHashMismatch,
    /// Code hash found in the pvmfw instance.img entry doesn't match the inputs.
    RecordedCodeHashMismatch,
    /// DICE mode found in the pvmfw instance.img entry doesn't match the current one.
    RecordedDiceModeMismatch,
    /// Size of the instance.img entry being read or written is not supported.
    UnsupportedEntrySize(usize),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::EmptyPvmfwEntry => write!(f, "Unexpected empty pvmfw instance.img entry"),
            Self::FailedIo(e) => write!(f, "Failed I/O to disk: {e}"),
            Self::FailedOpen(e_iter) => {
                writeln!(f, "Failed to open the instance.img partition:")?;
                for e in *e_iter {
                    writeln!(f, "\t{e}")?;
                }
                Ok(())
            }
            Self::FailedSeal(e_iter) => {
                writeln!(f, "Failed to seal the instance.img partition:")?;
                for e in *e_iter {
                    writeln!(f, "\t{e}")?;
                }
                Ok(())
            }
            Self::InstanceImageFull => write!(f, "Failed to obtain a free instance.img partition"),
            Self::InvalidInstanceImageHeader => write!(f, "instance.img header is invalid"),
            Self::MissingInstanceImage => write!(f, "Failed to find the instance.img partition"),
            Self::MissingInstanceImageHeader => write!(f, "instance.img header is missing"),
            Self::OverwritingEntry => write!(f, "Attempting to write to an existing entry"),
            Self::RecordedAuthHashMismatch => write!(f, "Recorded authority hash doesn't match"),
            Self::RecordedCodeHashMismatch => write!(f, "Recorded code hash doesn't match"),
            Self::RecordedDiceModeMismatch => write!(f, "Recorded DICE mode doesn't match"),
            Self::UnsupportedEntrySize(sz) => write!(f, "Invalid entry size: {sz}"),
        }
    }
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
pub enum Salt {
    New(Hidden),
    Found(Hidden),
}

pub fn get_instance_salt(
    pci_root: &mut PciRoot,
    dice_inputs: &PartialInputs,
    key: &[u8],
) -> Result<Salt> {
    let (mut partitions, instance_img) = find_instance_img(pci_root)?;
    let mut entry = Entry::locate_in(&mut partitions, &instance_img)?;

    let mut blk = [0; BLK_SIZE];
    if let Some(found) = entry.read_from(&mut partitions, &instance_img, key, &mut blk)? {
        let (blk_code_hash, blk_auth_hash, blk_salt, blk_mode) = split_entry(found);
        let mode = to_dice_mode(u8::from_le_bytes(blk_mode.try_into().unwrap()));

        if blk_code_hash != dice_inputs.code_hash {
            Err(Error::RecordedCodeHashMismatch)
        } else if blk_auth_hash != dice_inputs.auth_hash {
            Err(Error::RecordedAuthHashMismatch)
        } else if mode.ok_or(Error::RecordedDiceModeMismatch)? != dice_inputs.mode {
            Err(Error::RecordedDiceModeMismatch)
        } else {
            Ok(Salt::Found(blk_salt.try_into().unwrap()))
        }
    } else {
        let salt = [0; size_of::<Hidden>()]; // TODO(b/262393451): Generate using TRNG.

        let (blk_code_hash, blk_auth_hash, blk_salt, blk_mode) = split_entry_mut(&mut blk);
        blk_code_hash.copy_from_slice(&dice_inputs.code_hash);
        blk_auth_hash.copy_from_slice(&dice_inputs.auth_hash);
        blk_salt.copy_from_slice(&salt);
        blk_mode.copy_from_slice(&from_dice_mode(&dice_inputs.mode).to_le_bytes());

        let size = blk_code_hash.len() + blk_auth_hash.len() + blk_salt.len() + blk_mode.len();

        entry.write_to(&mut partitions, &instance_img, key, &blk[..size])?;

        Ok(Salt::New(salt))
    }
}

fn to_dice_mode(value: u8) -> Option<DiceMode> {
    match value {
        0 => Some(DiceMode::kDiceModeNotInitialized),
        1 => Some(DiceMode::kDiceModeNormal),
        2 => Some(DiceMode::kDiceModeDebug),
        3 => Some(DiceMode::kDiceModeMaintenance),
        _ => None,
    }
}

fn from_dice_mode(mode: &DiceMode) -> u8 {
    match mode {
        DiceMode::kDiceModeNotInitialized => 0,
        DiceMode::kDiceModeNormal => 1,
        DiceMode::kDiceModeDebug => 2,
        DiceMode::kDiceModeMaintenance => 3,
    }
}

#[repr(C, packed)]
struct Header {
    magic: [u8; Header::MAGIC.len()],
    version: u16,
}

impl Header {
    const MAGIC: &[u8] = b"Android-VM-instance";
    const VERSION_1: u16 = 1;

    pub fn is_valid(&self) -> bool {
        self.magic == Self::MAGIC && self.version() == Self::VERSION_1
    }

    fn version(&self) -> u16 {
        u16::from_le(self.version)
    }

    fn from_bytes(bytes: &[u8]) -> Option<&Self> {
        let header: &Self = bytes.as_ref();

        if header.is_valid() {
            Some(header)
        } else {
            None
        }
    }
}

impl AsRef<Header> for [u8] {
    fn as_ref(&self) -> &Header {
        // SAFETY - Assume that the alignement and size match Header.
        unsafe { &*self.as_ptr().cast::<Header>() }
    }
}

fn find_instance_img(pci_root: &mut PciRoot) -> Result<(Partitions, Partition)> {
    for device in VirtIOBlkIterator::new(pci_root) {
        if let Ok(mut parts) = Partitions::new(device) {
            match parts.get_partition_by_name("vm-instance") {
                Ok(Some(p)) => return Ok((parts, p)),
                Ok(None) => {}
                Err(e) => log::warn!("error while reading from disk: {e}"),
            };
        }
    }

    Err(Error::MissingInstanceImage)
}

#[derive(Debug)]
struct Entry {
    header_index: usize,
    payload_size: Option<NonZeroUsize>,
}

const BLK_SIZE: usize = Partitions::LBA_SIZE;

fn blk_count(a: usize) -> usize {
    a.checked_div(BLK_SIZE).unwrap() + usize::from(a.rem_euclid(BLK_SIZE) != 0)
}

impl Entry {
    const UUID: Uuid = Uuid::from_u128(0x90d2174a038a4bc6adf3824848fc5825);
    // TODO after upgrade of AOSP uuid: uuid!("90d2174a-038a-4bc6-adf3-824848fc5825")

    pub fn locate_in(parts: &mut Partitions, part: &Partition) -> Result<Self> {
        let mut blk = [0; BLK_SIZE];
        let mut indices = part.indices();
        let header_index = indices.next().ok_or(Error::MissingInstanceImageHeader)?;
        parts.read_partition_block(part, header_index, &mut blk).map_err(Error::FailedIo)?;
        #[allow(unused)] // The instance.img header is only used for discovery/validation.
        let header = Header::from_bytes(&blk).ok_or(Error::InvalidInstanceImageHeader)?;

        let mut skip = 0;
        for header_index in indices {
            if skip != 0 {
                skip -= 1;
                continue;
            }

            parts.read_partition_block(part, header_index, &mut blk).map_err(Error::FailedIo)?;
            let (blk_uuid, blk_size, _) = split_entry_header(&blk);
            let payload_size = u64::from_le_bytes(blk_size.try_into().unwrap());
            let payload_size = NonZeroUsize::new(usize::try_from(payload_size).unwrap());

            skip = match Uuid::from_slice(blk_uuid) {
                Err(_) => continue,
                Ok(Self::UUID) => {
                    log::trace!("Found pvmfw instance.img: {payload_size:?} bytes");
                    let payload_size = Some(payload_size.ok_or(Error::EmptyPvmfwEntry)?);
                    return Ok(Self { header_index, payload_size });
                }
                Ok(uuid) if uuid.is_nil() => return Ok(Self { header_index, payload_size: None }),
                Ok(uuid) => {
                    log::trace!("Skipping instance.img entry {uuid}: {payload_size:?} bytes");
                    payload_size.map(|sz| blk_count(sz.get())).unwrap_or(0)
                }
            };
        }

        Err(Error::InstanceImageFull)
    }

    pub fn read_from<'a>(
        &self,
        parts: &mut Partitions,
        part: &Partition,
        key: &[u8],
        entry: &'a mut [u8],
    ) -> Result<Option<&'a [u8]>> {
        let payload_size = if let Some(size) = self.payload_size {
            size.into()
        } else {
            return Ok(None);
        };

        let mut blk = [0; BLK_SIZE];
        if payload_size > blk.len() {
            // We currently only support single-blk entries.
            return Err(Error::UnsupportedEntrySize(payload_size));
        }

        parts
            .read_partition_block(part, self.payload_index(), &mut blk)
            .map_err(Error::FailedIo)?;
        let (encrypted, _) = blk.split_at(payload_size);

        let aead = AeadCtx::new_aes_256_gcm_randnonce(key).map_err(Error::FailedOpen)?;
        let decrypted = aead.open(entry, encrypted).map_err(Error::FailedOpen)?;

        Ok(Some(decrypted))
    }

    pub fn write_to(
        &mut self,
        parts: &mut Partitions,
        part: &Partition,
        key: &[u8],
        data: &[u8],
    ) -> Result<()> {
        if self.payload_size.is_some() {
            return Err(Error::OverwritingEntry);
        }

        let mut blk = [0; BLK_SIZE];

        let aead = AeadCtx::new_aes_256_gcm_randnonce(key).map_err(Error::FailedSeal)?;
        if blk.len() < data.len() + aead.aead().unwrap().max_overhead() {
            // We currently only support single-blk entries.
            return Err(Error::UnsupportedEntrySize(data.len()));
        }
        let encrypted = aead.seal(&mut blk, data).map_err(Error::FailedSeal)?;
        let payload_size = NonZeroUsize::new(encrypted.len()).ok_or(Error::EmptyPvmfwEntry)?;
        parts.write_partition_block(part, self.payload_index(), &blk).map_err(Error::FailedIo)?;

        let (blk_uuid, blk_size, blk_rest) = split_entry_header_mut(&mut blk);
        blk_uuid.copy_from_slice(Self::UUID.as_bytes());
        blk_size.copy_from_slice(&payload_size.get().to_le_bytes());
        blk_rest.fill(0);
        parts.write_partition_block(part, self.header_index, &blk).map_err(Error::FailedIo)?;

        self.payload_size = Some(payload_size);

        Ok(())
    }

    fn payload_index(&self) -> usize {
        self.header_index + 1
    }
}

fn split_entry_header(blk: &[u8]) -> (&[u8], &[u8], &[u8]) {
    let (blk_uuid, blk_rest) = blk.split_at(size_of::<u128>());
    let (blk_payload_size, blk_rest) = blk_rest.split_at(size_of::<u64>());

    (blk_uuid, blk_payload_size, blk_rest)
}

fn split_entry_header_mut(blk: &mut [u8]) -> (&mut [u8], &mut [u8], &mut [u8]) {
    let (blk_uuid, blk_rest) = blk.split_at_mut(size_of::<u128>());
    let (blk_payload_size, blk_rest) = blk_rest.split_at_mut(size_of::<u64>());

    (blk_uuid, blk_payload_size, blk_rest)
}

fn split_entry(blk: &[u8]) -> (&[u8], &[u8], &[u8], &[u8]) {
    let (blk_code_hash, blk_rest) = blk.split_at(size_of::<Hash>());
    let (blk_auth_hash, blk_rest) = blk_rest.split_at(size_of::<Hash>());
    let (blk_salt, blk_rest) = blk_rest.split_at(size_of::<Hidden>());
    let (blk_mode, _) = blk_rest.split_at(size_of::<u8>());

    (blk_code_hash, blk_auth_hash, blk_salt, blk_mode)
}

fn split_entry_mut(blk: &mut [u8]) -> (&mut [u8], &mut [u8], &mut [u8], &mut [u8]) {
    let (blk_code_hash, blk_rest) = blk.split_at_mut(size_of::<Hash>());
    let (blk_auth_hash, blk_rest) = blk_rest.split_at_mut(size_of::<Hash>());
    let (blk_salt, blk_rest) = blk_rest.split_at_mut(size_of::<Hidden>());
    let (blk_mode, _) = blk_rest.split_at_mut(size_of::<u8>());

    (blk_code_hash, blk_auth_hash, blk_salt, blk_mode)
}
