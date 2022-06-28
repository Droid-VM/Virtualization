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

//! Foobar

use libc::{c_char, c_int, c_void, size_t};
use core::fmt::{self, Display};
use core::iter::Iterator;
use core::option::Option;

// This links to libfdt which handles the creation of the binary blob
// flattened device tree (fdt) that is passed to the kernel and indicates
// the hardware configuration of the machine.
#[link(name = "fdt")]
extern "C" {
    fn fdt_check_full(buf: *const c_void, bufsize: size_t) -> c_int;
    fn fdt_path_offset_namelen(fdt: *const c_void, path: *const c_char, namelen : c_int) -> c_int;
    fn fdt_getprop_namelen(fdt: *const c_void, nodeoffset: c_int, name: *const c_char,
                           namelen: c_int, lenp: *mut c_int) -> *const c_void;
    fn fdt_address_cells(fdt: *const c_void, nodeoffset: c_int) -> c_int;
    fn fdt_size_cells(fdt: *const c_void, nodeoffset: c_int) -> c_int;
}

/// Foobar
#[derive(Debug)]
pub enum Error {
    /// Error from fdt_check_full function.
    FdtCheckFullError(c_int),
    /// Error from fdt_path_offset_namelen function.
    FdtPathOffsetNamelenError(c_int),
    /// Error from fdt_getprop_namelen function.
    FdtGetpropNamelenError(c_int),
    /// Error from fdt_address_cells function.
    FdtAddressCellsError(c_int),
    /// Error from fdt_size_cells function.
    FdtSizeCellsError(c_int),
    /// Foobar
    FdtUnexpectedValueError,
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Error::*;
        write!(f, "libfdt: ")?;
        match self {
            FdtCheckFullError(ret) => write!(f, "fdt_check_full returned {}", ret),
            FdtPathOffsetNamelenError(ret) => write!(f, "fdt_path_offset_namelen returned {}", ret),
            FdtGetpropNamelenError(ret) => write!(f, "fdt_getprop_namelen returned {}", ret),
            FdtAddressCellsError(ret) => write!(f, "fdt_address_cells returned {}", ret),
            FdtSizeCellsError(ret) => write!(f, "fdt_size_cells returned {}", ret),
            FdtUnexpectedValueError => write!(f, "unexpected value error"),
        }
    }
}

/// Foobar
pub type Result<T> = core::result::Result<T, Error>;

fn check_full(fdt: &[u8]) -> Result<()> {
    let ret = unsafe { fdt_check_full(fdt.as_ptr() as *const c_void, fdt.len()) };
    if ret != 0 {
        return Err(Error::FdtCheckFullError(ret));
    }
    Ok(())
}

fn get_path_offset(fdt: &[u8], path: &[c_char]) -> Result<usize> {
    let ret = unsafe { fdt_path_offset_namelen(fdt.as_ptr() as *const c_void,
                                               path.as_ptr(), path.len() as i32) };
    if ret < 0 {
        return Err(Error::FdtPathOffsetNamelenError(ret));
    }
    Ok(ret as usize)
}

fn get_prop<'a>(fdt: &'a [u8], nodeoffset: usize, name: &[c_char]) -> Result<&'a [u8]> {
    let mut lenp : c_int = 0;
    let ret = unsafe { fdt_getprop_namelen(fdt.as_ptr() as *const c_void,
                                           nodeoffset as c_int, name.as_ptr(),
                                           name.len() as c_int, &mut lenp as *mut c_int) };
    if ret.is_null() {
        return Err(Error::FdtGetpropNamelenError(lenp));
    }
    Ok(unsafe { core::slice::from_raw_parts(ret as *const u8, lenp as usize) })
}

fn get_address_cells(fdt: &[u8], nodeoffset: usize) -> Result<usize> {
    let ret = unsafe { fdt_address_cells(fdt.as_ptr() as *const c_void, nodeoffset as c_int ) };
    if ret < 0 {
        return Err(Error::FdtAddressCellsError(ret));
    }
    Ok(ret as usize)
}

fn get_size_cells(fdt: &[u8], nodeoffset: usize) -> Result<usize> {
    let ret = unsafe { fdt_size_cells(fdt.as_ptr() as *const c_void, nodeoffset as c_int ) };
    if ret < 0 {
        return Err(Error::FdtSizeCellsError(ret));
    }
    Ok(ret as usize)
}

const FDT_CELL_SIZE_BITS: usize = 32;
const FDT_CELL_SIZE : usize = core::mem::size_of::<u32>();
const FDT_MAX_CELLS : usize = core::mem::size_of::<usize>() / FDT_CELL_SIZE;

pub struct FdtCellIterator<'a> {
    remaining: &'a [u8],
}

impl<'a> FdtCellIterator<'a> {
    fn new(prop: &[u8]) -> FdtCellIterator {
        FdtCellIterator { remaining: prop }
    }
}

impl<'a> Iterator for FdtCellIterator<'a> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.len() < FDT_CELL_SIZE {
            return None;
        }

        let next = u32::from_be_bytes(self.remaining[..FDT_CELL_SIZE].try_into().unwrap());
        self.remaining = &self.remaining[FDT_CELL_SIZE..];
        Some(next)
    }
}

pub struct FdtRegIterator<'a> {
    iter: FdtCellIterator<'a>,
    addr_cells: usize,
    size_cells: usize,
}

pub struct FdtMemoryRegion {
    addr: usize,
    size: usize,
}

impl<'a> FdtRegIterator<'a> {
    fn new(fdt: &[u8], nodeoffset: usize) -> Result<FdtRegIterator> {
        let addr_cells = get_address_cells(fdt, nodeoffset)?;
        let size_cells = get_size_cells(fdt, nodeoffset)?;
        let reg = get_prop(fdt, nodeoffset, b"reg")?;

        if (addr_cells > FDT_MAX_CELLS) || (size_cells > FDT_MAX_CELLS) {
            return Err(Error::FdtUnexpectedValueError);
        }

        Ok(FdtRegIterator { iter: FdtCellIterator::new(reg), addr_cells, size_cells })
    }

    fn parse(&mut self, cells: usize) -> Option<usize> {
        let mut val : usize = 0;
        for _ in 0..cells {
            let next = self.iter.next()?;
            val <<= FDT_CELL_SIZE_BITS;
            val |= next as usize;
        }
        Some(val)
    }
}

impl<'a> Iterator for FdtRegIterator<'a> {
    type Item = FdtMemoryRegion;

    fn next(&mut self) -> Option<Self::Item> {
        let addr = self.parse(self.addr_cells)?;
        let size = self.parse(self.size_cells)?;
        Some(FdtMemoryRegion { addr, size })
    }
}

/// Foobar
pub struct FdtReader<'a> {
    fdt: &'a [u8],
}

impl<'a> FdtReader<'a> {
    /// Foobar
    pub fn new(fdt: &[u8]) -> Result<FdtReader> {
        check_full(fdt)?;
        Ok(FdtReader { fdt })
    }

    /// Foobar
    pub fn memory(&self) -> Result<FdtRegIterator> {
        let node = get_path_offset(self.fdt, b"/memory")?;
        if get_prop(self.fdt, node, b"device_type")? != b"memory\0" {
            return Err(Error::FdtUnexpectedValueError);
        }
        Ok(FdtRegIterator::new(self.fdt, node)?)
    }
}
