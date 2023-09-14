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

//! High-level FDT functions.

use core::ops::Range;
use libfdt::{Fdt, FdtError};
use vmbase::cstr;

/// Reads the DICE data range from the given `fdt`.
pub fn read_dice_range_from(fdt: &Fdt) -> libfdt::Result<Range<usize>> {
    let node = fdt.node(cstr!("/reserved-memory"))?.ok_or(FdtError::NotFound)?;
    let node = node.next_compatible(cstr!("google,open-dice"))?.ok_or(FdtError::NotFound)?;
    let reg = node.reg()?.ok_or(FdtError::NotFound)?.next().ok_or(FdtError::NotFound)?;

    let addr = to_usize(reg.addr)?;
    let size = to_usize(reg.size.ok_or(FdtError::NotFound)?)?;
    Ok(addr..usize_checked_add(addr, size)?)
}

fn to_usize<T: TryInto<usize>>(num: T) -> libfdt::Result<usize> {
    num.try_into().map_err(|_| FdtError::BadValue)
}

fn usize_checked_add(x: usize, y: usize) -> libfdt::Result<usize> {
    x.checked_add(y).ok_or(FdtError::BadValue)
}
