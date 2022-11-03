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

//! Memory management.

use crate::helpers;
use aarch64_paging::{
    idmap::IdMap,
    paging::{Attributes, MemoryRegion},
    MapError,
};
use core::ops;
use vmbase::layout;

// We assume that:
// - MAIR_EL1.Attr0 = "Device-nGnRE memory" (0b0000_0100)
// - MAIR_EL1.Attr1 = "Normal memory, Outer & Inner WB Non-transient, R/W-Allocate" (0b1111_1111)
const MEMORY: Attributes = Attributes::NORMAL.union(Attributes::NON_GLOBAL);
pub const DEVICE: Attributes = Attributes::DEVICE_NGNRE.union(Attributes::EXECUTE_NEVER);
pub const CODE: Attributes = MEMORY.union(Attributes::READ_ONLY);
pub const DATA: Attributes = MEMORY.union(Attributes::EXECUTE_NEVER);
pub const RODATA: Attributes = DATA.union(Attributes::READ_ONLY);

pub fn create_dynamic_table() -> Result<IdMap, MapError> {
    const ASID: usize = 1;
    const ROOT_LEVEL: usize = 1;

    let mut idmap = IdMap::new(ASID, ROOT_LEVEL);

    map_range(&mut idmap, layout::text_range(), CODE)?;
    map_range(&mut idmap, layout::rodata_range(), RODATA)?;
    map_range(&mut idmap, layout::writable_region(), DATA)?;

    idmap.activate();

    Ok(idmap)
}

pub fn map_range(
    idmap: &mut IdMap,
    r: ops::Range<usize>,
    attr: Attributes,
) -> Result<(), MapError> {
    idmap.map_range(&range_as_memory_region(r), attr)
}

pub fn map_slice(idmap: &mut IdMap, s: &[u8], attr: Attributes) -> Result<(), MapError> {
    idmap.map_range(&slice_as_memory_region(s), attr)
}

pub fn map_page(idmap: &mut IdMap, addr: usize, attr: Attributes) -> Result<(), MapError> {
    const PAGE_SIZE: usize = helpers::SIZE_4KB;
    let base = addr & !(PAGE_SIZE - 1); // TODO: helpers::align_down(addr, PAGE_SIZE);

    idmap.map_range(&MemoryRegion::new(base, base + PAGE_SIZE), attr)
}

// TODO: impl From<ops::Range<T>> for MemoryRegion
fn range_as_memory_region(r: ops::Range<usize>) -> MemoryRegion {
    MemoryRegion::new(r.start, r.end)
}

// TODO: impl From<&[T]> for MemoryRegion
fn slice_as_memory_region(s: &[u8]) -> MemoryRegion {
    let base = s.as_ptr() as usize;
    MemoryRegion::new(base, base + s.len())
}
