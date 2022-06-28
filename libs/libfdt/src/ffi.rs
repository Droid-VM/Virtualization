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

//! Foreign Function Interface for libfdt functions.
//! Exposes only bounds-checked functions wherever possible.

use core::ffi::c_void;

// C FFI types. Use until core::ffi::* becomes stable.
#[allow(non_camel_case_types)]
type c_int = i32;
#[allow(non_camel_case_types)]
type c_char = u8;
#[allow(non_camel_case_types)]
type c_size_t = usize;

#[derive(Debug)]
pub enum Error {
    NotFound,
    Exists,
    NoSpace,
    BadOffset,
    BadPath,
    BadPhandle,
    BadState,
    Truncated,
    BadMagic,
    BadVersion,
    BadStructure,
    BadLayout,
    Internal,
    BadNCells,
    BadValue,
    BadOverlay,
    NoPhandles,
    BadFlags,
    Alignment,
    Unknown(i32),
}

pub type Result<T> = core::result::Result<T, Error>;

fn fdt_err(val: c_int) -> Result<c_int> {
    if val >= 0 {
        Ok(val)
    } else {
        match val {
            -1 => Err(Error::NotFound),
            -2 => Err(Error::Exists),
            -3 => Err(Error::NoSpace),
            -4 => Err(Error::BadOffset),
            -5 => Err(Error::BadPath),
            -6 => Err(Error::BadPhandle),
            -7 => Err(Error::BadState),
            -8 => Err(Error::Truncated),
            -9 => Err(Error::BadMagic),
            -10 => Err(Error::BadVersion),
            -11 => Err(Error::BadStructure),
            -12 => Err(Error::BadLayout),
            -13 => Err(Error::Internal),
            -14 => Err(Error::BadNCells),
            -15 => Err(Error::BadValue),
            -16 => Err(Error::BadOverlay),
            -17 => Err(Error::NoPhandles),
            -18 => Err(Error::BadFlags),
            -19 => Err(Error::Alignment),
            _ => Err(Error::Unknown(val)),
        }
    }
}

#[link(name = "fdt")]
extern "C" {
    fn fdt_check_full(buf: *const c_void, bufsize: c_size_t) -> c_int;
    fn fdt_path_offset_namelen(fdt: *const c_void, path: *const c_char, namelen: c_int) -> c_int;
    fn fdt_parent_offset(fdt: *const c_void, nodeoffset: c_int) -> c_int;
    fn fdt_getprop_namelen(
        fdt: *const c_void,
        nodeoffset: c_int,
        name: *const c_char,
        namelen: c_int,
        lenp: *mut c_int,
    ) -> *const c_void;
    fn fdt_address_cells(fdt: *const c_void, nodeoffset: c_int) -> c_int;
    fn fdt_size_cells(fdt: *const c_void, nodeoffset: c_int) -> c_int;
}

pub fn check_full(fdt: &[u8]) -> Result<()> {
    let ret = unsafe { fdt_check_full(fdt.as_ptr() as *const c_void, fdt.len()) };
    fdt_err(ret).map(|_| ())
}

pub fn get_path_offset(fdt: &[u8], path: &[c_char]) -> Result<usize> {
    let ret = unsafe {
        fdt_path_offset_namelen(fdt.as_ptr() as *const c_void, path.as_ptr(), path.len() as i32)
    };
    Ok(fdt_err(ret)? as usize)
}

pub fn get_parent_offset(fdt: &[u8], nodeoffset: usize) -> Result<usize> {
    let ret = unsafe { fdt_parent_offset(fdt.as_ptr() as *const c_void, nodeoffset as c_int) };
    Ok(fdt_err(ret)? as usize)
}

pub fn get_prop<'a>(fdt: &'a [u8], nodeoffset: usize, name: &[c_char]) -> Result<&'a [u8]> {
    let mut lenp: c_int = 0;
    let ret = unsafe {
        fdt_getprop_namelen(
            fdt.as_ptr() as *const c_void,
            nodeoffset as c_int,
            name.as_ptr(),
            name.len() as c_int,
            &mut lenp as *mut c_int,
        )
    };
    if ret.is_null() {
        return Err(fdt_err(lenp).expect_err("fdt_getprop_namelen returned NULL"));
    }
    Ok(unsafe { core::slice::from_raw_parts(ret as *const u8, lenp as usize) })
}

pub fn get_address_cells(fdt: &[u8], nodeoffset: usize) -> Result<usize> {
    let ret = unsafe { fdt_address_cells(fdt.as_ptr() as *const c_void, nodeoffset as c_int) };
    Ok(fdt_err(ret)? as usize)
}

pub fn get_size_cells(fdt: &[u8], nodeoffset: usize) -> Result<usize> {
    let ret = unsafe { fdt_size_cells(fdt.as_ptr() as *const c_void, nodeoffset as c_int) };
    Ok(fdt_err(ret)? as usize)
}
