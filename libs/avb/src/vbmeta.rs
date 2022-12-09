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

//! A module to verify and parse VBMeta images with or without std.

use alloc::boxed::Box;
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
use core::ops::Deref;
use core::option::Option;
use core::ptr::{copy_nonoverlapping, null_mut};
use core::result::Result;

#[cfg(feature = "std")]
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

pub use crate::descriptor::{Descriptor, Descriptors};

/// Errors from parsing a VBMeta image.
#[derive(Debug)]
pub enum VbMetaImageParseError {
    /// There was an Io error parsing the VBMeta image.
    #[cfg(feature = "std")]
    Io(io::Error),
    /// The image footer was invalid.
    InvalidFooter,
    /// The image header was invalid.
    InvalidHeader,
    /// The image version is not supported.
    UnsupportedVersion,
    /// There was an invalid descriptor in the image.
    InvalidDescriptor,
}

impl fmt::Display for VbMetaImageParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            #[cfg(feature = "std")]
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::InvalidDescriptor => write!(f, "Invalid descriptor"),
            Self::InvalidHeader => write!(f, "Invalid header"),
            Self::InvalidFooter => write!(f, "Invalid footer"),
            Self::UnsupportedVersion => write!(f, "Unsupported version"),
        }
    }
}

/// Errors from verifying a VBMeta image.
#[derive(Debug)]
pub enum VbMetaImageVerificationError {
    /// There was an error parsing the VBMeta image.
    ParseError(VbMetaImageParseError),
    /// The VBMeta image hash did not validate.
    HashMismatch,
    /// The VBMeta image signature did not validate.
    SignatureMismatch,
    /// An unexpected libavb error code was returned.
    UnknownLibavbError(c_uint),
}

impl From<VbMetaImageParseError> for VbMetaImageVerificationError {
    fn from(error: VbMetaImageParseError) -> Self {
        VbMetaImageVerificationError::ParseError(error)
    }
}

impl fmt::Display for VbMetaImageVerificationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::ParseError(e) => write!(f, "Cannot parse VBMeta image: {e}"),
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
    /// Load and verify a VBMeta image from the given path.
    #[cfg(feature = "std")]
    pub fn verify_path<P: AsRef<Path>>(path: P) -> Result<Self, VbMetaImageVerificationError> {
        let file = File::open(path).map_err(VbMetaImageParseError::Io)?;
        let size = file.metadata().map_err(VbMetaImageParseError::Io)?.len();
        Self::verify_reader_region(file, 0, size)
    }

    /// Load and verify a VBMeta image from a region within a reader.
    #[cfg(feature = "std")]
    pub fn verify_reader_region<R: Read + Seek>(
        mut image: R,
        offset: u64,
        size: u64,
    ) -> Result<Self, VbMetaImageVerificationError> {
        // Check for a footer in the image or assume it's an entire VBMeta image.
        image.seek(SeekFrom::Start(offset + size)).map_err(VbMetaImageParseError::Io)?;
        let footer = read_avb_footer_from_image(&mut image).map_err(VbMetaImageParseError::Io)?;
        let (vbmeta_offset, vbmeta_size) = if let Some(footer) = footer {
            if footer.vbmeta_offset > size || footer.vbmeta_size > size - footer.vbmeta_offset {
                return Err(VbMetaImageParseError::InvalidFooter.into());
            }
            (footer.vbmeta_offset, footer.vbmeta_size)
        } else {
            (0, size)
        };
        image.seek(SeekFrom::Start(offset + vbmeta_offset)).map_err(VbMetaImageParseError::Io)?;
        // Verify the image before examining it to check the size.
        let mut data = vec![0u8; vbmeta_size as usize];
        image.read_exact(&mut data).map_err(VbMetaImageParseError::Io)?;
        Self::verify_vbmeta(&data)
    }

    /// Verifies the given vbmeta data.
    #[allow(dead_code)] // TODO(b/256148034): Remove this when pvmfw payload verification is code complete.
    fn verify_vbmeta(data: &[u8]) -> Result<Self, VbMetaImageVerificationError> {
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
        // TODO(b/256148034): Consider avoiding using the heap here.
        let data = data[..(vbmeta_size as usize)].to_vec();
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
#[allow(dead_code)] // TODO(b/256148034): Remove this when pvmfw payload verification is code complete.
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

/// Reads the AVB footer, if present, given a reader that's positioned at the end of the image.
#[cfg(feature = "std")]
fn read_avb_footer_from_image<R: Read + Seek>(image: &mut R) -> io::Result<Option<Footer>> {
    image.seek(SeekFrom::Current(-(size_of::<AvbFooter>() as i64)))?;
    let mut raw_footer = [0u8; size_of::<AvbFooter>()];
    image.read_exact(&mut raw_footer)?;
    Ok(Footer::try_from(&raw_footer).ok())
}

#[repr(transparent)]
struct Footer(AvbFooter);

impl TryFrom<&[u8; size_of::<AvbFooter>()]> for Footer {
    // TODO(b/256148034): Investigate if to raise VbMetaImageVerificationError here
    type Error = ();

    fn try_from(bytes: &[u8; size_of::<AvbFooter>()]) -> Result<Self, Self::Error> {
        // SAFETY: the slice is the same size as the struct which only contains simple data types.
        let mut footer = unsafe {
            let mut footer = MaybeUninit::<AvbFooter>::uninit();
            copy_nonoverlapping(
                bytes.as_ptr(),
                footer.as_mut_ptr() as *mut u8,
                size_of::<AvbFooter>(),
            );
            footer.assume_init()
        };
        // Check the magic matches "AVBf" to suppress misleading logs from libavb.
        const AVB_FOOTER_MAGIC: [u8; 4] = [0x41, 0x56, 0x42, 0x66];
        if footer.magic != AVB_FOOTER_MAGIC {
            return Err(());
        }
        // SAFETY: the function updates the struct in-place.
        if unsafe { avb_footer_validate_and_byteswap(&footer, &mut footer) } {
            Ok(Self(footer))
        } else {
            Err(())
        }
    }
}

impl Deref for Footer {
    type Target = AvbFooter;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{anyhow, Context, Result};
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::FileExt;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn unsigned_image_does_not_have_public_key() -> Result<()> {
        let test_dir = TempDir::new().unwrap();
        let test_file = test_dir.path().join("test.img");
        let mut cmd = Command::new("./avbtool");
        cmd.args([
            "make_vbmeta_image",
            "--output",
            test_file.to_str().unwrap(),
            "--algorithm",
            "NONE",
        ]);
        let status = cmd.status().context("make_vbmeta_image")?;
        assert!(status.success());
        let vbmeta = VbMetaImage::verify_path(test_file)
            .map_err(|e| anyhow!("verify path failed: {}", e))?;
        assert!(vbmeta.public_key().is_none());
        Ok(())
    }

    fn signed_image_has_valid_vbmeta(algorithm: &str, key: &str) -> Result<()> {
        let test_dir = TempDir::new().unwrap();
        let test_file = test_dir.path().join("test.img");
        let mut cmd = Command::new("./avbtool");
        cmd.args([
            "make_vbmeta_image",
            "--output",
            test_file.to_str().unwrap(),
            "--algorithm",
            algorithm,
            "--key",
            key,
        ]);
        let status = cmd.status().context("make_vbmeta_image")?;
        assert!(status.success());
        let vbmeta = VbMetaImage::verify_path(&test_file)
            .map_err(|e| anyhow!("verify path failed: {}", e))?;

        // The image should contain the public part of the key pair.
        let pubkey = vbmeta.public_key().unwrap();
        let test_pubkey_file = test_dir.path().join("test.pubkey");
        let mut cmd = Command::new("./avbtool");
        cmd.args([
            "extract_public_key",
            "--key",
            key,
            "--output",
            test_pubkey_file.to_str().unwrap(),
        ]);
        let status = cmd.status().context("extract_public_key")?;
        assert!(status.success());
        assert_eq!(pubkey, fs::read(test_pubkey_file).context("read public key")?);

        // Flip a byte to make verification fail.
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&test_file)
            .context("open image to flip byte")?;
        let mut data = [0; 1];
        file.read_exact_at(&mut data, 81).context("read byte from image to flip")?;
        data[0] = !data[0];
        file.write_all_at(&data, 81).context("write flipped byte to image")?;
        assert!(matches!(
            VbMetaImage::verify_path(test_file),
            Err(VbMetaImageVerificationError::HashMismatch)
        ));
        Ok(())
    }

    #[test]
    fn test_rsa2048_signed_image() -> Result<()> {
        signed_image_has_valid_vbmeta("SHA256_RSA2048", "data/testkey_rsa2048.pem")
    }

    #[test]
    fn test_rsa4096_signed_image() -> Result<()> {
        signed_image_has_valid_vbmeta("SHA256_RSA4096", "data/testkey_rsa4096.pem")
    }

    #[test]
    fn test_rsa8192_signed_image() -> Result<()> {
        signed_image_has_valid_vbmeta("SHA256_RSA8192", "data/testkey_rsa8192.pem")
    }
}
