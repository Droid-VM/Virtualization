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

mod ffi;

type FdtCellType = u32;
const FDT_CELL_BITS: usize = 32;
const FDT_CELL_BYTES: usize = core::mem::size_of::<FdtCellType>();
const FDT_MAX_CELLS: usize = core::mem::size_of::<usize>() / FDT_CELL_BYTES;

/// Error type for local functions.
#[derive(Debug)]
pub enum Error {
    /// Error returned from a low-level libfdt function.
    Libfdt(ffi::Error),
    /// Value of '#address-cells' property is not supported.
    UnexpectedAddressCellsValue,
    /// Value of '#size-cells' property is not supported.
    UnexpectedSizeCellsValue,
    /// Value of 'device-type' property in a memory node is not supported.
    UnexpectedMemoryDeviceType,
}

impl From<ffi::Error> for Error {
    fn from(err: ffi::Error) -> Self {
        Error::Libfdt(err)
    }
}

/// Result type with local Error type.
pub type Result<T> = core::result::Result<T, Error>;

/// Iterator over cells of a DT property.
pub struct CellIterator<'a> {
    remaining: &'a [u8],
}

impl<'a> CellIterator<'a> {
    fn new(fdt: &'a [u8], nodeoffset: usize, propname: &[u8]) -> Result<CellIterator<'a>> {
        Ok(CellIterator { remaining: ffi::get_prop(fdt, nodeoffset, propname)? })
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
    fn new(fdt: &[u8], node: usize) -> Result<RegIterator> {
        let parent = ffi::get_parent_offset(fdt, node)?;
        let addr_cells = ffi::get_address_cells(fdt, parent)?;
        let size_cells = ffi::get_size_cells(fdt, parent)?;

        if addr_cells > FDT_MAX_CELLS {
            return Err(Error::UnexpectedAddressCellsValue);
        }
        if size_cells > FDT_MAX_CELLS {
            return Err(Error::UnexpectedSizeCellsValue);
        }

        let iter = CellIterator::new(fdt, node, b"reg")?;
        Ok(RegIterator { iter, addr_cells, size_cells })
    }

    /// Parses a value of a given number of cells from the CellIterator.
    fn take_value(&mut self, cells: usize) -> Option<usize> {
        let mut val: usize = 0;
        for _ in 0..cells {
            val = (val << FDT_CELL_BITS) | (self.iter.next()? as usize);
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
        ffi::check_full(fdt)?;
        Ok(FdtReader { fdt })
    }

    /// Return an iterator of memory banks specified in "/memory" nodes.
    pub fn memory(&self) -> Result<RegIterator> {
        let node = ffi::get_path_offset(self.fdt, b"/memory")?;
        if ffi::get_prop(self.fdt, node, b"device_type")? != b"memory\0" {
            return Err(Error::UnexpectedMemoryDeviceType);
        }
        RegIterator::new(self.fdt, node)
    }
}
