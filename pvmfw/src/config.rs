// Copyright 2022, The Android Open Source Project
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

//! Support for the pvmfw configuration data format.

use core::fmt;
use core::mem;
use core::ops::Range;
use core::result;
use vmbase::util::{unchecked_align_up, RangeExt};
use zerocopy::{FromBytes, LayoutVerified};

/// Configuration data header.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, FromBytes)]
struct Header {
    /// Magic number; must be `Header::MAGIC`.
    magic: u32,
    /// Version of the header format.
    version: u32,
    /// Total size of the configuration data.
    total_size: u32,
    /// Feature flags; currently reserved and must be zero.
    flags: u32,
}

#[derive(Debug)]
pub enum Error {
    /// Reserved region can't fit configuration header.
    BufferTooSmall,
    /// Header has the wrong alignment
    HeaderMisaligned,
    /// Header doesn't contain the expect magic value.
    InvalidMagic,
    /// Version of the header isn't supported.
    UnsupportedVersion(u16, u16),
    /// Header sets flags incorrectly or uses reserved flags.
    InvalidFlags(u32),
    /// Header describes configuration data that doesn't fit in the expected buffer.
    InvalidSize(usize),
    /// Header entry is missing.
    MissingEntry(Entry),
    /// Range described by entry does not fit within config data.
    EntryOutOfBounds(Entry, Range<usize>, Range<usize>),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::BufferTooSmall => write!(f, "Reserved region is smaller than config header"),
            Self::HeaderMisaligned => write!(f, "Reserved region is misaligned"),
            Self::InvalidMagic => write!(f, "Wrong magic number"),
            Self::UnsupportedVersion(x, y) => write!(f, "Version {x}.{y} not supported"),
            Self::InvalidFlags(v) => write!(f, "Flags value {v:#x} is incorrect or reserved"),
            Self::InvalidSize(sz) => write!(f, "Total size ({sz:#x}) overflows reserved region"),
            Self::MissingEntry(entry) => write!(f, "Mandatory {entry:?} entry is missing"),
            Self::EntryOutOfBounds(entry, range, limits) => {
                write!(
                    f,
                    "Entry {entry:?} out of bounds: {range:#x?} must be within range {limits:#x?}"
                )
            }
        }
    }
}

pub type Result<T> = result::Result<T, Error>;

impl Header {
    const MAGIC: u32 = u32::from_ne_bytes(*b"pvmf");
    const VERSION_1_0: u32 = Self::version(1, 0);

    pub const fn version(major: u16, minor: u16) -> u32 {
        ((major as u32) << 16) | (minor as u32)
    }

    pub const fn version_tuple(&self) -> (u16, u16) {
        ((self.version >> 16) as u16, self.version as u16)
    }

    pub fn entry_count(&self) -> usize {
        Entry::COUNT
    }

    pub fn total_size(&self) -> usize {
        self.total_size as usize
    }

    pub fn body_offset(&self) -> usize {
        unchecked_align_up(mem::size_of::<Self>(), mem::size_of::<u64>())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Entry {
    Bcc = 0,
    DebugPolicy = 1,
}

impl Entry {
    const COUNT: usize = 2;
}

#[repr(packed)]
#[derive(Clone, Copy, Debug, FromBytes)]
struct HeaderEntry {
    offset: u32,
    size: u32,
}

impl HeaderEntry {
    pub fn as_range(&self) -> Option<Range<usize>> {
        let size = usize::try_from(self.size).unwrap();
        if size != 0 {
            let offset = self.offset.try_into().unwrap();
            Some(offset..(offset + size))
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub struct Config<'a> {
    body: &'a mut [u8],
    ranges: [Option<Range<usize>>; Entry::COUNT],
}

impl<'a> Config<'a> {
    /// Take ownership of a pvmfw configuration consisting of its header and following entries.
    pub fn new(bytes: &'a mut [u8]) -> Result<Self> {
        if bytes.len() < mem::size_of::<Header>() {
            return Err(Error::BufferTooSmall);
        }

        let (header, rest) =
            LayoutVerified::<_, Header>::new_from_prefix(bytes).ok_or(Error::HeaderMisaligned)?;
        let header = header.into_ref();

        if header.magic != Header::MAGIC {
            return Err(Error::InvalidMagic);
        }

        if header.version != Header::VERSION_1_0 {
            let (major, minor) = header.version_tuple();
            return Err(Error::UnsupportedVersion(major, minor));
        }

        if header.flags != 0 {
            return Err(Error::InvalidFlags(header.flags));
        }

        // Now that we can access the Header, validate its total_size and resize the byte slice.
        let total_size = header.total_size();
        let size_rest =
            total_size.checked_sub(header.body_offset()).ok_or(Error::InvalidSize(total_size))?;
        let rest = rest.get_mut(..size_rest).ok_or(Error::InvalidSize(total_size))?;

        let (header_entries, body) =
            LayoutVerified::<_, [HeaderEntry]>::new_slice_from_prefix(rest, header.entry_count())
                .ok_or(Error::BufferTooSmall)?;

        let limits = header.body_offset()..header.total_size();
        let ranges = [
            // TODO: Find a way to do this programmatically even if the trait
            // `core::marker::Copy` is not implemented for `core::ops::Range<usize>`
            Self::validated_body_range(Entry::Bcc, header_entries[0].as_range(), &limits)?,
            Self::validated_body_range(Entry::DebugPolicy, header_entries[1].as_range(), &limits)?,
        ];

        Ok(Self { body, ranges })
    }

    /// Get slice containing the platform BCC.
    pub fn get_entries(&mut self) -> Result<(&mut [u8], Option<&mut [u8]>)> {
        // This assumes that the blobs are in-order w.r.t. the entries.
        let bcc_range = self.get_entry_range(Entry::Bcc).ok_or(Error::MissingEntry(Entry::Bcc))?;
        let dp_range = self.get_entry_range(Entry::DebugPolicy);
        let bcc_start = bcc_range.start;
        let bcc_end = bcc_range.len();
        let (_, rest) = self.body.split_at_mut(bcc_start);
        let (bcc, rest) = rest.split_at_mut(bcc_end);

        let dp = if let Some(dp_range) = dp_range {
            let dp_start = dp_range.start.checked_sub(bcc_range.end).unwrap();
            let dp_end = dp_range.len();
            let (_, rest) = rest.split_at_mut(dp_start);
            let (dp, _) = rest.split_at_mut(dp_end);
            Some(dp)
        } else {
            None
        };

        Ok((bcc, dp))
    }

    pub fn get_entry_range(&self, entry: Entry) -> Option<Range<usize>> {
        self.ranges[entry as usize].clone()
    }

    fn validated_body_range(
        entry: Entry,
        range: Option<Range<usize>>,
        limits: &Range<usize>,
    ) -> Result<Option<Range<usize>>> {
        if let Some(r) = range {
            if r.is_within(limits) {
                let start = r.start.checked_sub(limits.start).unwrap();
                let end = r.end.checked_sub(limits.start).unwrap();

                Ok(Some(start..end))
            } else {
                Err(Error::EntryOutOfBounds(entry, r, limits.clone()))
            }
        } else {
            Ok(None)
        }
    }
}
