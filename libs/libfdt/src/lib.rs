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

use core::ffi::{c_int, c_void, CStr};
use core::fmt;
use core::marker;
use core::mem;
use core::ops;
use core::result;
use core::slice;

/// Error type corresponding to libfdt error codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
            Self::Unknown(e) => write!(f, "Unknown libfdt error '{e}'"),
        }
    }
}

/// Result type with FdtError enum.
pub type Result<T> = result::Result<T, FdtError>;

fn fdt_err(val: c_int) -> Result<c_int> {
    if val >= 0 {
        Ok(val)
    } else {
        Err(match -val as _ {
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
}

fn fdt_err_expect_zero(val: c_int) -> Result<()> {
    match fdt_err(val)? {
        0 => Ok(()),
        _ => Err(FdtError::Unknown(val)),
    }
}

/// Non-negative offset of a node in a DT that can safely be used as a c_int.
///
/// Its generic lifetime is used to track the lifetime of its associated DT.
#[repr(transparent)]
#[derive(Copy, Clone, Debug)]
pub struct NodeOffset<'a>(c_int, marker::PhantomData<Fdt<'a>>);

impl TryFrom<c_int> for NodeOffset<'_> {
    type Error = FdtError;

    fn try_from(res: c_int) -> Result<Self> {
        Ok(Self(fdt_err(res)?, marker::PhantomData))
    }
}

impl From<NodeOffset<'_>> for c_int {
    fn from(node: NodeOffset) -> Self {
        node.0
    }
}

/// Iterator over cells of a DT property.
#[derive(Debug)]
pub struct CellIterator<'a> {
    chunks: slice::ChunksExact<'a, u8>,
}

impl<'a> CellIterator<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        const CHUNK_SIZE: usize = mem::size_of::<<CellIterator as Iterator>::Item>();

        Self { chunks: bytes.chunks_exact(CHUNK_SIZE) }
    }
}

impl<'a> Iterator for CellIterator<'a> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.chunks.next()?;
        Some(Self::Item::from_be_bytes(bytes.try_into().ok()?))
    }
}

/// Size of a IEEE1275-compliant multi-cell DT property value.
#[derive(Copy, Clone, Debug)]
enum NCells {
    None = 0,
    Single = 1,
    Double = 2,
}

impl TryFrom<c_int> for NCells {
    type Error = FdtError;

    fn try_from(res: c_int) -> Result<Self> {
        match fdt_err(res)? {
            x if x == Self::None as c_int => Ok(Self::None),
            x if x == Self::Single as c_int => Ok(Self::Single),
            x if x == Self::Double as c_int => Ok(Self::Double),
            _ => Err(FdtError::BadNCells),
        }
    }
}

impl From<NCells> for usize {
    fn from(ncells: NCells) -> Self {
        ncells as usize
    }
}

/// Iterator over a 'reg' property of a DT node.
#[derive(Debug)]
pub struct RegIterator<'a> {
    cells: CellIterator<'a>,
    addr_cells: NCells,
    size_cells: NCells,
}

/// Represents a contiguous region within the address space defined by the parent bus.
/// Commonly means the offsets and lengths of MMIO blocks, but may have a different meaning on some
/// bus types. Addresses in the address space defined by the root node are CPU real addresses.
#[derive(Copy, Clone, Debug)]
pub struct Reg<T> {
    /// Base address of the region.
    pub addr: T,
    /// Size of the region (optional).
    pub size: Option<T>,
}

impl<'a> RegIterator<'a> {
    fn new(fdt: &Fdt<'a>, node: NodeOffset) -> Result<Self> {
        let parent = fdt.parent_offset(node)?;
        let addr_cells = fdt.address_cells(parent)?;
        let size_cells = fdt.size_cells(parent)?;

        // A value of 0 for #address-cells doesn't seem to be permitted by the specification.
        if matches!(addr_cells, NCells::None) {
            return Err(FdtError::BadNCells);
        }

        let prop_name = CStr::from_bytes_with_nul(b"reg\0").unwrap();
        let cells = CellIterator::new(fdt.getprop(node, prop_name)?);
        Ok(Self { cells, addr_cells, size_cells })
    }

    /// Parses a value of a given number of cells from the CellIterator.
    fn take_ncells(&mut self, ncells: NCells) -> Option<u64> {
        const BITS_PER_CELL: usize = mem::size_of::<<CellIterator as Iterator>::Item>() * 8;
        match ncells {
            NCells::None => None,
            NCells::Single => Some(self.cells.next()?.into()),
            NCells::Double => {
                let first = u64::from(self.cells.next()?);
                Some((first << (BITS_PER_CELL as u32)) | u64::from(self.cells.next()?))
            }
        }
    }
}

impl<'a> Iterator for RegIterator<'a> {
    type Item = Reg<u64>;

    fn next(&mut self) -> Option<Self::Item> {
        let addr = self.take_ncells(self.addr_cells)?;
        // If the parent node specifies a value of 0 for #size-cells, 'size' shall be omitted.
        let size = self.take_ncells(self.size_cells);
        Some(Self::Item { addr, size })
    }
}

/// Iterator over the address ranges defined by the /memory/ node.
#[derive(Debug)]
pub struct MemRegIterator<'a> {
    reg: RegIterator<'a>,
}

impl<'a> MemRegIterator<'a> {
    fn new(fdt: &Fdt<'a>) -> Result<Self> {
        let path = CStr::from_bytes_with_nul(b"/memory\0").unwrap();
        let device_type = CStr::from_bytes_with_nul(b"device_type\0").unwrap();
        let node = fdt.path_offset(path)?;
        if fdt.getprop(node, device_type)? != b"memory\0" {
            return Err(FdtError::BadValue);
        }
        Ok(Self { reg: RegIterator::new(fdt, node)? })
    }
}

impl<'a> Iterator for MemRegIterator<'a> {
    type Item = ops::Range<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.reg.next()?;
        let addr = usize::try_from(next.addr).ok()?;
        let size = usize::try_from(next.size?).ok()?;

        Some(addr..addr.checked_add(size)?)
    }
}

/// Wrapper around low-level read-only libfdt functions.
#[repr(transparent)]
pub struct Fdt<'a> {
    fdt: &'a [u8],
}

impl<'a> Fdt<'a> {
    /// Wraps a slice containing a Flattened Device Tree.
    ///
    /// Fails if the FDT does not pass validation.
    pub fn new(fdt: &'a [u8]) -> Result<Self> {
        let fdt = Self { fdt };
        fdt.check_full()?;
        Ok(fdt)
    }

    /// Return an iterator of memory banks specified the "/memory" node.
    ///
    /// NOTE: This does not support individual "/memory@XXXX" banks.
    pub fn memory(&self) -> Result<MemRegIterator> {
        MemRegIterator::new(self)
    }

    fn check_full(&self) -> Result<()> {
        let len = self.fdt.len();
        let ret = unsafe { libfdt_bindgen::fdt_check_full(self.as_ptr(), len) };
        fdt_err_expect_zero(ret)
    }

    fn path_offset(&self, path: &CStr) -> Result<NodeOffset> {
        let len = path.to_bytes().len().try_into().map_err(|_| FdtError::BadPath)?;
        let ret = unsafe {
            // *_namelen functions don't include the trailing nul terminator in 'len'.
            libfdt_bindgen::fdt_path_offset_namelen(self.as_ptr(), path.as_ptr(), len)
        };

        ret.try_into()
    }

    fn getprop(&self, node: NodeOffset, name: &CStr) -> Result<&'a [u8]> {
        let mut len: i32 = 0;
        let prop = unsafe {
            libfdt_bindgen::fdt_getprop_namelen(
                self.as_ptr(),
                node.into(),
                name.as_ptr(),
                // *_namelen functions don't include the trailing nul terminator in 'len'.
                name.to_bytes().len().try_into().map_err(|_| FdtError::BadPath)?,
                &mut len as *mut i32,
            )
        } as *const u8;
        if prop.is_null() {
            return fdt_err(len).and(Err(FdtError::Internal));
        }
        let len = usize::try_from(fdt_err(len)?).map_err(|_| FdtError::Internal)?;
        let base = (prop as usize).checked_sub(self.as_ptr() as usize).ok_or(FdtError::Internal)?;

        self.fdt.get(base..(base + len)).ok_or(FdtError::Internal)
    }

    fn parent_offset(&self, node: NodeOffset) -> Result<NodeOffset> {
        unsafe { libfdt_bindgen::fdt_parent_offset(self.as_ptr(), node.into()) }.try_into()
    }

    fn address_cells(&self, node: NodeOffset) -> Result<NCells> {
        unsafe { libfdt_bindgen::fdt_address_cells(self.as_ptr(), node.into()) }.try_into()
    }

    fn size_cells(&self, node: NodeOffset) -> Result<NCells> {
        unsafe { libfdt_bindgen::fdt_size_cells(self.as_ptr(), node.into()) }.try_into()
    }

    fn as_ptr(&self) -> *const c_void {
        self.fdt.as_ptr() as *const c_void
    }
}

/// Wrapper around low-level read-write libfdt functions.
#[repr(transparent)]
pub struct FdtMut<'a> {
    fdt: &'a mut [u8],
}

impl<'a> FdtMut<'a> {
    /// Wraps a slice containing a modifiable Flattened Device Tree.
    pub fn new(fdt: &'a mut [u8]) -> Result<Self> {
        Ok(Self { fdt })
    }

    /// Make the whole slice containing the DT available to libfdt.
    pub fn unpack(&mut self) -> Result<()> {
        let ret = unsafe {
            libfdt_bindgen::fdt_open_into(
                self.as_ptr(),
                self.as_mut_ptr(),
                self.fdt.len().try_into().map_err(|_| FdtError::Internal)?,
            )
        };
        fdt_err_expect_zero(ret)
    }

    /// Pack the DT to take a minimum amount of memory.
    ///
    /// Doesn't shrink the underlying memory slice.
    pub fn pack(&mut self) -> Result<()> {
        let ret = unsafe { libfdt_bindgen::fdt_pack(self.as_mut_ptr()) };
        fdt_err_expect_zero(ret)
    }

    /// Add a new subnode to the given node.
    pub fn add_subnode(&mut self, parent: NodeOffset, name: &CStr) -> Result<NodeOffset> {
        let ret = unsafe {
            libfdt_bindgen::fdt_add_subnode(self.as_mut_ptr(), parent.into(), name.as_ptr())
        };

        ret.try_into()
    }

    /// Append a property name-value (possibly empty) pair to the given node.
    pub fn appendprop(&mut self, node: NodeOffset, name: &CStr, value: &[u8]) -> Result<()> {
        let ret = unsafe {
            libfdt_bindgen::fdt_appendprop(
                self.as_mut_ptr(),
                node.into(),
                name.as_ptr(),
                value.as_ptr().cast::<c_void>(),
                value.len().try_into().map_err(|_| FdtError::BadValue)?,
            )
        };

        fdt_err_expect_zero(ret)
    }

    /// Append a (address, size) pair property to the given node.
    pub fn appendprop_addrrange(
        &mut self,
        node: NodeOffset,
        name: &CStr,
        addr: u64,
        size: u64,
    ) -> Result<()> {
        let ret = unsafe {
            libfdt_bindgen::fdt_appendprop_addrrange(
                self.as_mut_ptr(),
                self.parent_offset(node)?.into(),
                node.into(),
                name.as_ptr(),
                addr,
                size,
            )
        };

        fdt_err_expect_zero(ret)
    }

    /// Get the offset of the node at the given path.
    pub fn path_offset(&self, path: &CStr) -> Result<NodeOffset> {
        let len = path.to_bytes().len().try_into().map_err(|_| FdtError::BadPath)?;
        let ret = unsafe {
            // *_namelen functions don't include the trailing nul terminator in 'len'.
            libfdt_bindgen::fdt_path_offset_namelen(self.as_ptr(), path.as_ptr(), len)
        };

        ret.try_into()
    }

    fn parent_offset(&self, node: NodeOffset) -> Result<NodeOffset> {
        unsafe { libfdt_bindgen::fdt_parent_offset(self.as_ptr(), node.into()) }.try_into()
    }

    fn as_mut_ptr(&mut self) -> *mut c_void {
        self.fdt.as_mut_ptr() as *mut c_void
    }

    fn as_ptr(&self) -> *const c_void {
        self.fdt.as_ptr() as *const c_void
    }
}

impl<'a> TryFrom<FdtMut<'a>> for Fdt<'a> {
    type Error = FdtError;

    fn try_from(fdt: FdtMut<'a>) -> Result<Self> {
        Self::new(fdt.fdt)
    }
}
