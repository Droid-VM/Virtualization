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

use crate::helpers;
use core::mem;
use core::num::NonZeroUsize;
use core::ops;
use core::slice;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

#[repr(packed)]
#[derive(Clone, Copy, Debug)]
struct Header {
    magic: u32,
    version: u32,
    total_size: u32,
    flags: u32,
    entries: [HeaderEntry; 2],
}

impl Header {
    const MAGIC: u32 = u32::from_ne_bytes(*b"pvmf");
    const PADDED_SIZE: usize = helpers::align(mem::size_of::<Self>(), mem::size_of::<u64>());

    pub const fn version(major: u16, minor: u16) -> u32 {
        ((major as u32) << 16) | (minor as u32)
    }

    pub fn total_size(&self) -> usize {
        self.total_size as usize
    }

    pub fn body_size(&self) -> usize {
        self.total_size() - Self::PADDED_SIZE
    }

    unsafe fn body_ptr(&self) -> *const u8 {
        (self as *const Self).cast::<u8>().add(Self::PADDED_SIZE)
    }

    fn is_valid(&self, max_size: usize) -> bool {
        if self.magic != Self::MAGIC || self.version != Self::version(1, 0) || self.flags != 0 {
            return false;
        }

        let total_size = self.total_size();

        total_size <= max_size && self.entries.into_iter().all(|e| e.is_valid(total_size))
    }

    fn get(&self, entry: Entry) -> HeaderEntry {
        self.entries[entry as usize]
    }
}

enum Entry {
    Bcc = 0,
    DebugPolicy = 1,
}

#[repr(packed)]
#[derive(Clone, Copy, Debug)]
struct HeaderEntry {
    offset: u32,
    size: u32,
}

impl HeaderEntry {
    pub fn is_valid(&self, max_size: usize) -> bool {
        (Header::PADDED_SIZE..max_size).contains(&self.offset())
            && NonZeroUsize::new(self.size())
                .and_then(|s| s.checked_add(self.offset()))
                .filter(|&x| x.get() <= max_size)
                .is_some()
    }

    pub fn as_body_range(&self) -> ops::Range<usize> {
        let start = self.offset() - Header::PADDED_SIZE;

        start..(start + self.size())
    }

    pub fn offset(&self) -> usize {
        self.offset as usize
    }

    pub fn size(&self) -> usize {
        self.size as usize
    }
}

#[derive(Debug)]
pub struct Config<'a> {
    header: &'a mut Header,
    body: &'a mut [u8],
}

impl<'a> Config<'a> {
    /// Take ownership of a pvmfw configuration consisting of its header and following entries.
    ///
    /// SAFETY: This constructor takes ownership of the entries appended to the header.
    unsafe fn new(header: &'a mut Header) -> Self {
        let body = slice::from_raw_parts_mut(header.body_ptr().cast_mut(), header.body_size());

        Self { body, header }
    }

    /// Get slice containing the platform BCC.
    pub fn get_bcc_mut(&mut self) -> &mut [u8] {
        &mut self.body[self.header.get(Entry::Bcc).as_body_range()]
    }

    /// Get slice containing the platform debug policy.
    #[allow(dead_code)] // TODO(b/232900974)
    pub fn get_debug_policy(&self) -> &[u8] {
        &self.body[self.header.get(Entry::DebugPolicy).as_body_range()]
    }
}

unsafe fn get_header() -> &'static mut Header {
    &mut *(helpers::locate_appended_payload() as *mut Header)
}

/// Get a unique reference to the configuration data.
pub fn take() -> Option<Config<'static>> {
    static TAKEN: AtomicBool = AtomicBool::new(false);

    if TAKEN.swap(true, Ordering::Relaxed) {
        return None;
    }

    // SAFETY - This function is the only way to access the payload.
    let header = unsafe { get_header() };

    if header.is_valid(helpers::max_appended_payload_size()) {
        // SAFETY - At this point, the region of the payload following the header isn't owned.
        Some(unsafe { Config::new(header) })
    } else {
        None
    }
}
