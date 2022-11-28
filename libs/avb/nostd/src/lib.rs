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

//! A library to verify and parse VBMeta images without std

#![no_std]

extern crate alloc;

mod descriptor;

use alloc::boxed::Box;
use alloc::string::String;
use avb_bindgen::{
    avb_footer_validate_and_byteswap, avb_vbmeta_image_header_to_host_byte_order,
    avb_vbmeta_image_verify, AvbAlgorithmType_AVB_ALGORITHM_TYPE_NONE, AvbFooter,
    AvbVBMetaImageHeader, AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_HASH_MISMATCH,
    AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_INVALID_VBMETA_HEADER,
    AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_OK,
    AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_OK_NOT_SIGNED,
    AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_SIGNATURE_MISMATCH,
    AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_UNSUPPORTED_VERSION,
};
use core::ffi::c_uint;
use core::fmt;
use core::mem::{size_of, MaybeUninit};
use core::option::Option;
use core::ptr::null_mut;
use core::result::Result;
use core::slice;

pub use crate::descriptor::{Descriptor, Descriptors};

/// Errors from parsing a VBMeta image.
#[derive(Debug)]
pub enum VbMetaImageParseError {
    /// There was an invalid descriptor in the image.
    InvalidDescriptor,
}

impl fmt::Display for VbMetaImageParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidDescriptor => write!(f, "Invalid descriptor"),
        }
    }
}

/// Errors from verifying a VBMeta image.
#[derive(Debug)]
pub enum VbMetaImageVerificationError {
    /// There was an Io error parsing the VBMeta image.
    Io(String),
    /// The image header was invalid.
    InvalidHeader,
    /// The image footer was invalid.
    InvalidFooter,
    /// The image version is not supported.
    UnsupportedVersion,
    /// The VBMeta image hash did not validate.
    HashMismatch,
    /// The VBMeta image signature did not validate.
    SignatureMismatch,
    /// An unexpected libavb error code was returned.
    UnknownLibavbError(c_uint),
}

impl fmt::Display for VbMetaImageVerificationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Cannot parse VBMeta image: {:?}", e),
            Self::InvalidHeader => write!(f, "Invalid header"),
            Self::InvalidFooter => write!(f, "Invalid footer"),
            Self::UnsupportedVersion => write!(f, "Unsupported version"),
            Self::HashMismatch => write!(f, "Hash mismatch"),
            Self::SignatureMismatch => write!(f, "Signature mismatch"),
            Self::UnknownLibavbError(e) => write!(f, "Unknown libavb error: {e}"),
        }
    }
}

/// A VBMeta Image.
pub struct VbMetaImage {
    header: AvbVBMetaImageHeader,
    data: Box<[u8]>,
}

impl VbMetaImage {
    /// Verifies the given vbmeta data.
    pub fn verify_vbmeta(data: &[u8]) -> Result<Self, VbMetaImageVerificationError> {
        verify_vbmeta_image(data)?;
        // SAFETY: the image has been verified so we know there is a valid header at the start.
        let header = unsafe {
            let mut header = MaybeUninit::uninit();
            let src = data.as_ptr() as *const _ as *const AvbVBMetaImageHeader;
            avb_vbmeta_image_header_to_host_byte_order(src, header.as_mut_ptr());
            header.assume_init()
        };
        // Calculate the true size of the verified image data.
        let vbmeta_size = (size_of::<AvbVBMetaImageHeader>() as u64)
            + header.authentication_data_block_size
            + header.auxiliary_data_block_size;
        let vbmeta = data[..(vbmeta_size as usize)].to_vec();
        Ok(Self { header, data: vbmeta.into_boxed_slice() })
    }

    /// Get the public key that verified the VBMeta image. If the image was not signed, there
    /// is no such public key.
    pub fn public_key(&self) -> Option<&[u8]> {
        if self.header.algorithm_type == AvbAlgorithmType_AVB_ALGORITHM_TYPE_NONE {
            return None;
        }
        let begin = size_of::<AvbVBMetaImageHeader>()
            + self.header.authentication_data_block_size as usize
            + self.header.public_key_offset as usize;
        let end = begin + self.header.public_key_size as usize;
        Some(&self.data[begin..end])
    }

    /// Get the hash of the verified data in the VBMeta image from the authentication block. If the
    /// image was not signed, there might not be a hash and, if there is, it's not known to be
    /// correct.
    pub fn hash(&self) -> Option<&[u8]> {
        if self.header.algorithm_type == AvbAlgorithmType_AVB_ALGORITHM_TYPE_NONE {
            return None;
        }
        let begin = size_of::<AvbVBMetaImageHeader>() + self.header.hash_offset as usize;
        let end = begin + self.header.hash_size as usize;
        Some(&self.data[begin..end])
    }

    /// Get the descriptors of the VBMeta image.
    pub fn descriptors(&self) -> Result<Descriptors<'_>, VbMetaImageParseError> {
        Descriptors::from_image(&self.data)
    }

    /// Get the raw VBMeta image.
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

/// Verify the data as a VBMeta image, translating errors that arise.
fn verify_vbmeta_image(data: &[u8]) -> Result<(), VbMetaImageVerificationError> {
    // SAFETY: the function only reads from the provided data and the NULL pointers disable the
    // output arguments.
    let res = unsafe { avb_vbmeta_image_verify(data.as_ptr(), data.len(), null_mut(), null_mut()) };
    #[allow(non_upper_case_globals)]
    match res {
        AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_OK
        | AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_OK_NOT_SIGNED => Ok(()),
        AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_INVALID_VBMETA_HEADER => {
            Err(VbMetaImageVerificationError::InvalidHeader)
        }
        AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_UNSUPPORTED_VERSION => {
            Err(VbMetaImageVerificationError::UnsupportedVersion)
        }
        AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_HASH_MISMATCH => {
            Err(VbMetaImageVerificationError::HashMismatch)
        }
        AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_SIGNATURE_MISMATCH => {
            Err(VbMetaImageVerificationError::SignatureMismatch)
        }
        err => Err(VbMetaImageVerificationError::UnknownLibavbError(err)),
    }
}

/// Reads AVB footer from the raw image data.
#[allow(dead_code)] // This method will be used for kernel image verification in pvmfw
fn read_avb_footer(image: &[u8]) -> Option<AvbFooter> {
    // SAFETY: the slice is the same size as the struct which only contains simple data types.
    let footer = unsafe {
        let mut footer = MaybeUninit::<AvbFooter>::uninit();
        let footer_slice =
            slice::from_raw_parts_mut(&mut footer as *mut _ as *mut u8, size_of::<AvbFooter>());
        footer_slice.copy_from_slice(&image[(image.len() - size_of::<AvbFooter>())..]);
        footer.assume_init()
    };
    validate_avb_footer(footer)
}

/// Validates AVB footer.
pub fn validate_avb_footer(mut footer: AvbFooter) -> Option<AvbFooter> {
    // Check the magic matches "AVB" to suppress misleading logs from libavb.
    const AVB_FOOTER_MAGIC: [u8; 4] = [0x41, 0x56, 0x42, 0x66];
    if footer.magic != AVB_FOOTER_MAGIC {
        return None;
    }
    // SAFETY: the function updates the struct in-place.
    if unsafe { avb_footer_validate_and_byteswap(&footer, &mut footer) } {
        Some(footer)
    } else {
        None
    }
}
