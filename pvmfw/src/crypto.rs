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

//! Wrapper around BoringSSL/OpenSSL symbols.

use core::convert::AsRef;
use core::ffi::{c_char, c_int, c_void, CStr};
use core::fmt;
use core::mem::MaybeUninit;
use core::num::NonZeroU32;
use core::ptr;

#[derive(Debug)]
pub struct Error {
    packed: NonZeroU32,
    file: Option<&'static CStr>,
    line: c_int,
}

impl Error {
    fn get() -> Option<Self> {
        let mut file = MaybeUninit::uninit();
        let mut line = MaybeUninit::uninit();
        // SAFETY - The function writes to the provided pointers, validated below.
        let packed = unsafe { ERR_get_error_line(file.as_mut_ptr(), line.as_mut_ptr()) };
        // SAFETY - Any possible value returned could be considered a valid *const c_char.
        let file = unsafe { file.assume_init() };
        // SAFETY - Any possible value returned could be considered a valid c_int.
        let line = unsafe { line.assume_init() };

        let packed = packed.try_into().ok()?;
        // SAFETY - Any non-NULL result is expected to point to a global const C string.
        let file = unsafe { as_static_cstr(file) };

        Some(Self { packed, file, line })
    }

    fn packed_value(&self) -> u32 {
        self.packed.get()
    }

    fn library_name(&self) -> Option<&'static CStr> {
        // SAFETY - Call to a pure function.
        let name = unsafe { ERR_lib_error_string(self.packed_value()) };
        // SAFETY - Any non-NULL result is expected to point to a global const C string.
        unsafe { as_static_cstr(name) }
    }

    fn reason(&self) -> Option<&'static CStr> {
        // SAFETY - Call to a pure function.
        let reason = unsafe { ERR_reason_error_string(self.packed_value()) };
        // SAFETY - Any non-NULL result is expected to point to a global const C string.
        unsafe { as_static_cstr(reason) }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let unknown_library = CStr::from_bytes_with_nul(b"{unknown library}\0").unwrap();
        let unknown_reason = CStr::from_bytes_with_nul(b"{unknown reason}\0").unwrap();
        let unknown_file = CStr::from_bytes_with_nul(b"??\0").unwrap();

        let packed = self.packed_value();
        let library = self.library_name().unwrap_or(unknown_library).to_str().unwrap();
        let reason = self.reason().unwrap_or(unknown_reason).to_str().unwrap();
        let file = self.file.unwrap_or(unknown_file).to_str().unwrap();
        let line = self.line;

        write!(f, "{file}:{line}: {library}: {reason} ({packed:#x})")
    }
}

#[derive(Copy, Clone)]
pub struct ErrorIterator {}

impl Iterator for ErrorIterator {
    type Item = Error;

    fn next(&mut self) -> Option<Self::Item> {
        Self::Item::get()
    }
}

pub type Result<T> = core::result::Result<T, ErrorIterator>;

#[repr(transparent)]
pub struct Aead(EVP_AEAD);

impl Aead {
    pub fn aes_256_gcm_randnonce() -> Option<&'static Self> {
        // SAFETY - Returned pointer is checked below.
        let aead = unsafe { EVP_aead_aes_256_gcm_randnonce() };
        if aead.is_null() {
            None
        } else {
            // SAFETY - We assume that the non-NULL value points to a valid and static EVP_AEAD.
            Some(unsafe { &*(aead as *const _) })
        }
    }

    pub fn max_overhead(&self) -> usize {
        // SAFETY - Function should only read from self.
        unsafe { EVP_AEAD_max_overhead(self.as_ref() as *const _) }
    }
}

#[repr(transparent)]
pub struct AeadCtx(EVP_AEAD_CTX);

impl AeadCtx {
    pub fn new_aes_256_gcm_randnonce(key: &[u8]) -> Result<Self> {
        let aead = Aead::aes_256_gcm_randnonce().unwrap();

        Self::new(aead, key)
    }

    fn new(aead: &'static Aead, key: &[u8]) -> Result<Self> {
        const DEFAULT_TAG_LENGTH: usize = 0;
        let engine = ptr::null_mut(); // Use default implementation.
        let mut ctx = MaybeUninit::zeroed();
        // SAFETY - Initialize the EVP_AEAD_CTX with const pointers to the AEAD and key.
        let result = unsafe {
            EVP_AEAD_CTX_init(
                ctx.as_mut_ptr(),
                aead.as_ref() as *const _,
                key.as_ptr(),
                key.len(),
                DEFAULT_TAG_LENGTH,
                engine,
            )
        };

        if result == 0 {
            Err(ErrorIterator {})
        } else {
            // SAFETY - We assume that the non-NULL value points to a valid and static EVP_AEAD.
            Ok(Self(unsafe { ctx.assume_init() }))
        }
    }

    pub fn aead(&self) -> Option<&'static Aead> {
        // SAFETY - The function should only read from self.
        let aead = unsafe { EVP_AEAD_CTX_aead(self.as_ref() as *const _) };
        if aead.is_null() {
            None
        } else {
            // SAFETY - We assume that the non-NULL value points to a valid and static EVP_AEAD.
            Some(unsafe { &*(aead as *const _) })
        }
    }

    pub fn open<'b>(&self, out: &'b mut [u8], data: &[u8]) -> Result<&'b mut [u8]> {
        let nonce = ptr::null_mut();
        let nonce_len = 0;
        let ad = ptr::null_mut();
        let ad_len = 0;
        let mut out_len = MaybeUninit::uninit();
        // SAFETY - The function should only read from self and write to out (at most the provided
        // number of bytes) and out_len while reading from data (at most the provided number of
        // bytes), ignoring any NULL input.
        let result = unsafe {
            EVP_AEAD_CTX_open(
                self.as_ref() as *const _,
                out.as_mut_ptr(),
                out_len.as_mut_ptr(),
                out.len(),
                nonce,
                nonce_len,
                data.as_ptr(),
                data.len(),
                ad,
                ad_len,
            )
        };

        if result == 0 {
            Err(ErrorIterator {})
        } else {
            // SAFETY - Any value written to out_len could be a valid usize. The value itself is
            // validated as being a proper slice length by panicking in the following indexing
            // otherwise.
            let out_len = unsafe { out_len.assume_init() };
            Ok(&mut out[..out_len])
        }
    }

    pub fn seal<'b>(&self, out: &'b mut [u8], data: &[u8]) -> Result<&'b mut [u8]> {
        let nonce = ptr::null_mut();
        let nonce_len = 0;
        let ad = ptr::null_mut();
        let ad_len = 0;
        let mut out_len = MaybeUninit::uninit();
        // SAFETY - The function should only read from self and write to out (at most the provided
        // number of bytes) while reading from data (at most the provided number of bytes),
        // ignoring any NULL input.
        let result = unsafe {
            EVP_AEAD_CTX_seal(
                self.as_ref() as *const _,
                out.as_mut_ptr(),
                out_len.as_mut_ptr(),
                out.len(),
                nonce,
                nonce_len,
                data.as_ptr(),
                data.len(),
                ad,
                ad_len,
            )
        };

        if result == 0 {
            Err(ErrorIterator {})
        } else {
            // SAFETY - Any value written to out_len could be a valid usize. The value itself is
            // validated as being a proper slice length by panicking in the following indexing
            // otherwise.
            let out_len = unsafe { out_len.assume_init() };
            Ok(&mut out[..out_len])
        }
    }
}

/// Cast a C string pointer to a static non-mutable reference.
///
/// # Safety
///
/// The caller needs to ensure that the pointer points to a valid C string and that the C lifetime
/// of the string is compatible with a static Rust lifetime.
unsafe fn as_static_cstr(p: *const c_char) -> Option<&'static CStr> {
    if p.is_null() {
        None
    } else {
        Some(CStr::from_ptr(p))
    }
}

#[allow(non_camel_case_types)]
#[repr(transparent)]
struct EVP_AEAD(c_void);

impl AsRef<EVP_AEAD> for Aead {
    fn as_ref(&self) -> &EVP_AEAD {
        &self.0
    }
}

#[allow(non_camel_case_types)]
#[repr(C, packed)]
struct EVP_AEAD_CTX {
    aead: *const EVP_AEAD,
    state: [u8; 580], // This field is a C union; replace it with its largest member here.
    tag_len: u64,     // This seems to be a `uint8_t` with `alignof(uint64_t)`.
}

impl AsRef<EVP_AEAD_CTX> for AeadCtx {
    fn as_ref(&self) -> &EVP_AEAD_CTX {
        &self.0
    }
}

pub fn hkdf_sh512<const N: usize>(secret: &[u8], salt: &[u8], info: &[u8]) -> Result<[u8; N]> {
    let mut key = [0; N];
    // SAFETY - The function shouldn't access any Rust variable and the returned value is accepted
    // as a potentially NULL pointer.
    let digest = unsafe { EVP_sha512() };
    // SAFETY - Only reads from/writes to the provided slices and supports digest.is_null().
    let result = unsafe {
        HKDF(
            key.as_mut_ptr(),
            key.len(),
            digest,
            secret.as_ptr(),
            secret.len(),
            salt.as_ptr(),
            salt.len(),
            info.as_ptr(),
            info.len(),
        )
    };

    if result == 0 {
        Err(ErrorIterator {})
    } else {
        Ok(key)
    }
}

#[allow(non_camel_case_types)]
#[repr(C, packed)]
struct EVP_MD {
    opaque: [u8; 0],
}

extern "C" {
    fn HKDF(
        out_key: *mut u8,
        out_len: usize,
        digest: *const EVP_MD,
        secret: *const u8,
        secret_len: usize,
        salt: *const u8,
        salt_len: usize,
        info: *const u8,
        info_len: usize,
    ) -> c_int;

    fn EVP_sha512() -> *const EVP_MD;
    fn EVP_aead_aes_256_gcm_randnonce() -> *const EVP_AEAD;
    fn EVP_AEAD_max_overhead(aead: *const EVP_AEAD) -> usize;

    fn EVP_AEAD_CTX_aead(ctx: *const EVP_AEAD_CTX) -> *const EVP_AEAD;
    fn EVP_AEAD_CTX_init(
        ctx: *mut EVP_AEAD_CTX,
        aead: *const EVP_AEAD,
        key: *const u8,
        key_len: usize,
        tag_len: usize,
        engine: *mut c_void,
    ) -> c_int;
    fn EVP_AEAD_CTX_open(
        ctx: *const EVP_AEAD_CTX,
        out: *mut u8,
        out_len: *mut usize,
        max_out_len: usize,
        nonce: *const u8,
        nonce_len: usize,
        data: *const u8,
        data_len: usize,
        ad: *const u8,
        ad_len: usize,
    ) -> c_int;

    fn EVP_AEAD_CTX_seal(
        ctx: *const EVP_AEAD_CTX,
        out: *mut u8,
        out_len: *mut usize,
        max_out_len: usize,
        nonce: *const u8,
        nonce_len: usize,
        data: *const u8,
        data_len: usize,
        ad: *const u8,
        ad_len: usize,
    ) -> c_int;

    fn ERR_get_error_line(file: *mut *const c_char, line: *mut c_int) -> u32;
    fn ERR_lib_error_string(packed: u32) -> *const c_char;
    fn ERR_reason_error_string(packed: u32) -> *const c_char;
}
