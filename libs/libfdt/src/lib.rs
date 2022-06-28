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

//! Wrapper around libfdt library. Provides parsing/generating functionality
//! to a bare-metal environment.

#![no_std]
#![feature(core_ffi_c)]

use core::ffi::{c_char, c_int, c_void};
use core::fmt;

/// Error type corresponding to libfdt error codes.
#[derive(Clone, Debug)]
pub enum FdtError {
    /// FDT_ERR_NOTFOUND
    NotFound,
    /// FDT_ERR_EXISTS
    Exists,
    /// FDT_ERR_NOSPACE
    NoSpace,
    /// FDT_ERR_BADOFFSET
    BadOffset,
    /// FDT_ERR_BADPATH
    BadPath,
    /// FDT_ERR_BADPHANDLE
    BadPhandle,
    /// FDT_ERR_BADSTATE
    BadState,
    /// FDT_ERR_TRUNCATED
    Truncated,
    /// FDT_ERR_BADMAGIC
    BadMagic,
    /// FDT_ERR_BADVERSION
    BadVersion,
    /// FDT_ERR_BADSTRUCTURE
    BadStructure,
    /// FDT_ERR_BADLAYOUT
    BadLayout,
    /// FDT_ERR_INTERNAL
    Internal,
    /// FDT_ERR_BADNCELLS
    BadNCells,
    /// FDT_ERR_BADVALUE
    BadValue,
    /// FDT_ERR_BADOVERLAY
    BadOverlay,
    /// FDT_ERR_NOPHANDLES
    NoPhandles,
    /// FDT_ERR_BADFLAGS
    BadFlags,
    /// FDT_ERR_ALIGNMENT
    Alignment,
    /// Unexpected error code
    Unknown(i32),
}

impl fmt::Display for FdtError {
    /// Prints error messages from libfdt.h documentation.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "The requested node or property does not exist"),
            Self::Exists => write!(f, "Attempted to create an existing node or property"),
            Self::NoSpace => write!(f, "Insufficient buffer space to contain the expanded tree"),
            Self::BadOffset => write!(f, "Structure block offset is out-of-bounds or invalid"),
            Self::BadPath => write!(f, "Badly formatted path"),
            Self::BadPhandle => write!(f, "Invalid phandle length or value"),
            Self::BadState => write!(f, "Received incomplete device tree"),
            Self::Truncated => write!(f, "Device tree or sub-block is improperly terminated"),
            Self::BadMagic => write!(f, "Device tree header missing its magic number"),
            Self::BadVersion => write!(f, "Device tree has a version which can't be handled"),
            Self::BadStructure => write!(f, "Device tree has a corrupt structure block"),
            Self::BadLayout => write!(f, "Device tree sub-blocks in unsupported order"),
            Self::Internal => write!(f, "libfdt has failed an internal assertion"),
            Self::BadNCells => write!(f, "Bad format or value of #address-cells or #size-cells"),
            Self::BadValue => write!(f, "Unexpected property value"),
            Self::BadOverlay => write!(f, "Overlay cannot be applied"),
            Self::NoPhandles => write!(f, "Device tree doesn't have any phandle available anymore"),
            Self::BadFlags => write!(f, "Invalid flag or invalid combination of flags"),
            Self::Alignment => write!(f, "Device tree base address is not 8-byte aligned"),
            Self::Unknown(e) => write!(f, "Unknown libfdt error '{}'", e),
        }
    }
}

/// Result type with Error enum.
pub type Result<T> = core::result::Result<T, FdtError>;

fn fdt_err(val: i32) -> Result<i32> {
    if val >= 0 {
        return Ok(val);
    }
    Err(match -val as u32 {
        libfdt_bindgen::FDT_ERR_NOTFOUND => FdtError::NotFound,
        libfdt_bindgen::FDT_ERR_EXISTS => FdtError::Exists,
        libfdt_bindgen::FDT_ERR_NOSPACE => FdtError::NoSpace,
        libfdt_bindgen::FDT_ERR_BADOFFSET => FdtError::BadOffset,
        libfdt_bindgen::FDT_ERR_BADPATH => FdtError::BadPath,
        libfdt_bindgen::FDT_ERR_BADPHANDLE => FdtError::BadPhandle,
        libfdt_bindgen::FDT_ERR_BADSTATE => FdtError::BadState,
        libfdt_bindgen::FDT_ERR_TRUNCATED => FdtError::Truncated,
        libfdt_bindgen::FDT_ERR_BADMAGIC => FdtError::BadMagic,
        libfdt_bindgen::FDT_ERR_BADVERSION => FdtError::BadVersion,
        libfdt_bindgen::FDT_ERR_BADSTRUCTURE => FdtError::BadStructure,
        libfdt_bindgen::FDT_ERR_BADLAYOUT => FdtError::BadLayout,
        libfdt_bindgen::FDT_ERR_INTERNAL => FdtError::Internal,
        libfdt_bindgen::FDT_ERR_BADNCELLS => FdtError::BadNCells,
        libfdt_bindgen::FDT_ERR_BADVALUE => FdtError::BadValue,
        libfdt_bindgen::FDT_ERR_BADOVERLAY => FdtError::BadOverlay,
        libfdt_bindgen::FDT_ERR_NOPHANDLES => FdtError::NoPhandles,
        libfdt_bindgen::FDT_ERR_BADFLAGS => FdtError::BadFlags,
        libfdt_bindgen::FDT_ERR_ALIGNMENT => FdtError::Alignment,
        _ => FdtError::Unknown(val),
    })
}

type FdtCellType = u32;
type FdtNodeOffset = c_int;
type FdtCString = [c_char];

const FDT_CELL_BITS: u32 = 32;
const FDT_CELL_BYTES: usize = core::mem::size_of::<FdtCellType>();
const FDT_MAX_CELLS: usize = core::mem::size_of::<usize>() / FDT_CELL_BYTES;

fn to_ptr(fdt: &[u8]) -> *const c_void {
    fdt.as_ptr() as *const c_void
}

fn check_full(fdt: &[u8]) -> Result<()> {
    let ret = unsafe { libfdt_bindgen::fdt_check_full(to_ptr(fdt), fdt.len()) };
    let ret = fdt_err(ret)?;
    if ret == 0 {
        Ok(())
    } else {
        Err(FdtError::Unknown(ret))
    }
}

fn path_offset(fdt: &[u8], path: &FdtCString) -> Result<FdtNodeOffset> {
    let ret = unsafe {
        libfdt_bindgen::fdt_path_offset_namelen(to_ptr(fdt), path.as_ptr(), path.len() as i32)
    };
    fdt_err(ret)
}

fn parent_offset(fdt: &[u8], nodeoffset: FdtNodeOffset) -> Result<FdtNodeOffset> {
    let ret = unsafe { libfdt_bindgen::fdt_parent_offset(to_ptr(fdt), nodeoffset) };
    fdt_err(ret)
}

fn getprop<'a>(fdt: &'a [u8], nodeoffset: FdtNodeOffset, name: &FdtCString) -> Result<&'a [u8]> {
    let mut lenp: i32 = 0;
    let ret = unsafe {
        libfdt_bindgen::fdt_getprop_namelen(
            to_ptr(fdt),
            nodeoffset,
            name.as_ptr(),
            name.len() as i32,
            &mut lenp as *mut i32,
        )
    };
    if ret.is_null() {
        return Err(fdt_err(lenp).expect_err("fdt_getprop_namelen returned NULL"));
    }
    Ok(unsafe {
        core::slice::from_raw_parts(
            ret as *const u8,
            usize::try_from(lenp).map_err(|_| FdtError::BadValue)?,
        )
    })
}

fn address_cells(fdt: &[u8], nodeoffset: FdtNodeOffset) -> Result<usize> {
    let ret = unsafe { libfdt_bindgen::fdt_address_cells(to_ptr(fdt), nodeoffset) };
    let val = fdt_err(ret)?;
    Ok(val as usize)
}

fn size_cells(fdt: &[u8], nodeoffset: FdtNodeOffset) -> Result<usize> {
    let ret = unsafe { libfdt_bindgen::fdt_size_cells(to_ptr(fdt), nodeoffset) };
    let val = fdt_err(ret)?;
    Ok(val as usize)
}

/// Iterator over cells of a DT property.
pub struct CellIterator<'a> {
    remaining: &'a [u8],
}

impl<'a> CellIterator<'a> {
    fn new(
        fdt: &'a [u8],
        nodeoffset: FdtNodeOffset,
        propname: &FdtCString,
    ) -> Result<CellIterator<'a>> {
        Ok(CellIterator { remaining: getprop(fdt, nodeoffset, propname)? })
    }
}

impl<'a> Iterator for CellIterator<'a> {
    type Item = FdtCellType;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.len() < FDT_CELL_BYTES {
            return None;
        }

        let bytes: [u8; FDT_CELL_BYTES] = self.remaining[..FDT_CELL_BYTES].try_into().unwrap();
        let val = FdtCellType::from_be_bytes(bytes);
        self.remaining = &self.remaining[FDT_CELL_BYTES..];
        Some(val)
    }
}

/// Iterator over a 'reg' property of a DT node.
pub struct RegIterator<'a> {
    iter: CellIterator<'a>,
    addr_cells: usize,
    size_cells: usize,
}

/// Represents a single, contiguous memory region parsed from the DT.
/// Does not specify whether the address is a PA/IPA/VA.
/// Does not check for integer overflow of base+size.
#[derive(Debug)]
pub struct MemRegion {
    /// Base address of a memory region.
    pub addr: usize,
    /// Size of a memory region.
    pub size: usize,
}

impl<'a> RegIterator<'a> {
    fn new(fdt: &[u8], nodeoffset: FdtNodeOffset) -> Result<RegIterator> {
        let parent = parent_offset(fdt, nodeoffset)?;
        let addr_cells = address_cells(fdt, parent)?;
        let size_cells = size_cells(fdt, parent)?;

        if addr_cells > FDT_MAX_CELLS || size_cells > FDT_MAX_CELLS {
            return Err(FdtError::BadNCells);
        }

        let iter = CellIterator::new(fdt, nodeoffset, b"reg")?;
        Ok(RegIterator { iter, addr_cells, size_cells })
    }

    /// Parses a value of a given number of cells from the CellIterator.
    fn take_value(&mut self, cells: usize) -> Option<usize> {
        let mut val: usize = 0;
        for _ in 0..cells {
            val = val.checked_shl(FDT_CELL_BITS)? | self.iter.next()? as usize;
        }
        Some(val)
    }
}

impl<'a> Iterator for RegIterator<'a> {
    type Item = MemRegion;

    fn next(&mut self) -> Option<Self::Item> {
        let addr = self.take_value(self.addr_cells)?;
        let size = self.take_value(self.size_cells)?;
        Some(MemRegion { addr, size })
    }
}

/// Wrapper around low-level libfdt functions.
pub struct FdtReader<'a> {
    fdt: &'a [u8],
}

impl<'a> FdtReader<'a> {
    /// Create an FdtReader for a given FDT slice.
    ///
    /// Fails if the FDT does not pass validation.
    pub fn new(fdt: &[u8]) -> Result<FdtReader> {
        check_full(fdt)?;
        Ok(FdtReader { fdt })
    }

    /// Return an iterator of memory banks specified in "/memory" nodes.
    pub fn memory(&self) -> Result<RegIterator> {
        let node = path_offset(self.fdt, b"/memory")?;
        if getprop(self.fdt, node, b"device_type")? != b"memory\0" {
            return Err(FdtError::BadValue);
        }
        RegIterator::new(self.fdt, node)
    }
}
