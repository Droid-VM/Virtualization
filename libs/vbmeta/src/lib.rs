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

//! A library to verify and parse VBMeta images.

mod descriptor;

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
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::mem::{size_of, MaybeUninit};
use std::os::raw::c_uint;
use std::path::Path;
use std::ptr::null_mut;
use std::slice;
use thiserror::Error;

pub use crate::descriptor::{Descriptor, Descriptors};

/// Errors from parsing a VBMeta image.
#[derive(Debug, Error)]
pub enum VbMetaImageParseError {
    /// There was an IO error.
    #[error("IO error")]
    Io(#[from] io::Error),
    /// The image is invalid.
    #[error("Invalid")]
    Invalid,
    /// The image header was invalid.
    #[error("Invalid header")]
    InvalidHeader,
    /// The image version is not supported.
    #[error("Unsupported version")]
    UnsupportedVersion,
    /// There was an invalid descriptor in the image.
    #[error("Invalid descriptor ")]
    InvalidDescriptor,
}

/// Errors from verifying a VBMeta image.
#[derive(Debug, Error)]
pub enum VbMetaImageVerificationError {
    /// There was an error parsing the VBMeta image.
    #[error("Cannot parse VBMeta image")]
    ParseError(#[from] VbMetaImageParseError),
    /// The VBMeta image hash did not validate.
    #[error("Hash mismatch")]
    HashMismatch,
    /// The VBMeta image signature did not validate.
    #[error("Signature mismatch")]
    SignatureMismatch,
    /// An unexpected libavb error code was returned.
    #[error("Unknown libavb error: {0}")]
    UnknownLibavbError(c_uint),
}

/// A VBMeta Image.
pub struct VbMetaImage {
    header: AvbVBMetaImageHeader,
    data: Box<[u8]>,
}

impl VbMetaImage {
    /// Load and verify a VBMeta image from the given path.
    pub fn verify_path<P: AsRef<Path>>(path: P) -> Result<Self, VbMetaImageVerificationError> {
        let file = File::open(path).map_err(VbMetaImageParseError::Io)?;
        let size = file.metadata().map_err(VbMetaImageParseError::Io)?.len();
        Self::verify_reader_region(file, 0, size)
    }

    /// Load and verify a VBMeta image from a region within a reader.
    pub fn verify_reader_region<R: Read + Seek>(
        mut image: R,
        offset: u64,
        size: u64,
    ) -> Result<Self, VbMetaImageVerificationError> {
        // Check for a footer in the image or assume it's an entire VBMeta image.
        image.seek(SeekFrom::Start(offset + size)).map_err(VbMetaImageParseError::Io)?;
        let footer = read_avb_footer(&mut image).map_err(VbMetaImageParseError::Io)?;
        let (vbmeta_offset, vbmeta_size) = if let Some(footer) = footer {
            (footer.vbmeta_offset, footer.vbmeta_size)
        } else {
            (0, size)
        };
        image.seek(SeekFrom::Start(offset + vbmeta_offset)).map_err(VbMetaImageParseError::Io)?;
        // Verify the image before examining it to check the size.
        let mut data = vec![0u8; vbmeta_size as usize];
        image.read_exact(&mut data).map_err(VbMetaImageParseError::Io)?;
        verify_vbmeta_image(&data)?;
        // SAFETY: the image has been verified so we know there is a valid header at the start.
        let header = unsafe {
            let mut header = MaybeUninit::uninit();
            let src = data.as_ptr() as *const _ as *const AvbVBMetaImageHeader;
            avb_vbmeta_image_header_to_host_byte_order(src, header.as_mut_ptr());
            header.assume_init()
        };
        let vbmeta_size = (size_of::<AvbVBMetaImageHeader>() as u64)
            + header.authentication_data_block_size
            + header.auxiliary_data_block_size;
        data.truncate(vbmeta_size as usize);
        Ok(Self { header, data: data.into_boxed_slice() })
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
            Err(VbMetaImageParseError::InvalidHeader.into())
        }
        AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_UNSUPPORTED_VERSION => {
            Err(VbMetaImageParseError::UnsupportedVersion.into())
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

/// Read the AVB footer given a reader that's positioned at the end of the image.
fn read_avb_footer<R: Read + Seek>(image: &mut R) -> io::Result<Option<AvbFooter>> {
    image.seek(SeekFrom::Current(-(size_of::<AvbFooter>() as i64)))?;
    // SAFETY: the slice is the same size as the struct which only contains simple data types.
    let mut footer = unsafe {
        let mut footer = MaybeUninit::<AvbFooter>::uninit();
        let footer_slice =
            slice::from_raw_parts_mut(&mut footer as *mut _ as *mut u8, size_of::<AvbFooter>());
        image.read_exact(footer_slice)?;
        footer.assume_init()
    };
    // Check the magic matches "AVBf" to supress misleading logs from libavb.
    const AVB_FOOTER_MAGIC: [u8; 4] = [0x41, 0x56, 0x42, 0x66];
    if footer.magic != AVB_FOOTER_MAGIC {
        return Ok(None);
    }
    // SAFETY: the function updates the struct in-place.
    if unsafe { avb_footer_validate_and_byteswap(&footer, &mut footer) } {
        Ok(Some(footer))
    } else {
        Ok(None)
    }
}
