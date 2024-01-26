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
mod ops;

use crate::ops::Ops;
use avb::{
    slot_verify, Descriptor, DescriptorError, HashtreeErrorMode, SlotVerifyData, SlotVerifyError,
    SlotVerifyFlags,
};
use std::fs::File;
use std::io::{self, Read, Seek};
use std::path::Path;
use thiserror::Error;

/// Errors from parsing a VBMeta image.
#[derive(Debug, Error)]
pub enum VbMetaImageParseError {
    /// There was an IO error.
    #[error("IO error")]
    Io(#[from] io::Error),
    /// The image footer was invalid.
    #[error("Invalid footer")]
    InvalidFooter,
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
    /// Couldn't find a VBMeta image in the provided data.
    #[error("Failed to locate VBMeta image")]
    MissingVbMetaImage,
}

/// The libavb errors don't map exactly to our library errors; it may be worth considering
/// just returning libavb errors directly instead.
impl<'a> From<SlotVerifyError<'a>> for VbMetaImageVerificationError {
    fn from(error: SlotVerifyError<'a>) -> Self {
        match error {
            SlotVerifyError::InvalidArgument => {
                VbMetaImageParseError::Io(io::Error::from(io::ErrorKind::InvalidInput)).into()
            }
            // TODO: the rest of these, and make IO error conversion easier.

            //     SlotVerifyError::InvalidMetadata => VbMetaImageParseError::InvalidHeader.into(),
            //     SlotVerifyError::Io => VbMetaImageParseError::Io.into(),
            // Oom,
            // PublicKeyRejected,
            // RollbackIndex,
            // UnsupportedVersion,
            // Verification(Option<SlotVerifyData<'a>>),
            // Internal,
            _ => Self::HashMismatch,
        }
    }
}

// TODO: consider returning `DescriptorError` directly.
impl From<DescriptorError> for VbMetaImageParseError {
    fn from(_: DescriptorError) -> Self {
        // We don't care about the fine-grained descriptor error.
        Self::InvalidDescriptor
    }
}

/// A VBMeta Image.
pub struct VbMetaImage {
    verify_data: SlotVerifyData<'static>,
    public_key: Vec<u8>,
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
        image: R,
        offset: u64,
        size: u64,
    ) -> Result<Self, VbMetaImageVerificationError> {
        // Use libavb `slot_verify()` to read and verify the vbmeta blob in `image`. We don't
        // have any actual partitions to validate but we can pass an empty partition list to
        // just process the vbmeta data.
        let mut ops = Ops { image, offset, size, public_key: Default::default() };
        let verify_data = slot_verify(
            &mut ops,
            &[],  // requested_partitions
            None, // ab_suffix
            SlotVerifyFlags::AVB_SLOT_VERIFY_FLAGS_NONE,
            HashtreeErrorMode::AVB_HASHTREE_ERROR_MODE_EIO,
        )?;

        // This library expects exactly one vbmeta blob in `image`.
        if verify_data.vbmeta_data().len() != 1 {
            return Err(VbMetaImageVerificationError::MissingVbMetaImage);
        }

        Ok(Self { verify_data, public_key: ops.public_key })
    }

    /// Get the public key that verified the VBMeta image. If the image was not signed, there
    /// is no such public key.
    pub fn public_key(&self) -> Option<&[u8]> {
        if self.public_key.is_empty() {
            return None;
        }
        Some(&self.public_key)
    }

    /// Get the descriptors of the VBMeta image.
    pub fn descriptors(&self) -> Result<Vec<Descriptor>, VbMetaImageParseError> {
        Ok(self.verify_data.vbmeta_data()[0].descriptors()?)
    }

    /// Get the raw VBMeta image.
    pub fn data(&self) -> &[u8] {
        self.verify_data.vbmeta_data()[0].data()
    }
}

// /// Verify the data as a VBMeta image, translating errors that arise.
// fn verify_vbmeta_image(data: &[u8]) -> Result<(), VbMetaImageVerificationError> {
//     // SAFETY: the function only reads from the provided data and the NULL pointers disable the
//     // output arguments.
//     let res = unsafe { avb_vbmeta_image_verify(data.as_ptr(), data.len(), null_mut(), null_mut())
// };     match res {
//         AvbVBMetaVerifyResult::AVB_VBMETA_VERIFY_RESULT_OK
//         | AvbVBMetaVerifyResult::AVB_VBMETA_VERIFY_RESULT_OK_NOT_SIGNED => Ok(()),
//         AvbVBMetaVerifyResult::AVB_VBMETA_VERIFY_RESULT_INVALID_VBMETA_HEADER => {
//             Err(VbMetaImageParseError::InvalidHeader.into())
//         }
//         AvbVBMetaVerifyResult::AVB_VBMETA_VERIFY_RESULT_UNSUPPORTED_VERSION => {
//             Err(VbMetaImageParseError::UnsupportedVersion.into())
//         }
//         AvbVBMetaVerifyResult::AVB_VBMETA_VERIFY_RESULT_HASH_MISMATCH => {
//             Err(VbMetaImageVerificationError::HashMismatch)
//         }
//         AvbVBMetaVerifyResult::AVB_VBMETA_VERIFY_RESULT_SIGNATURE_MISMATCH => {
//             Err(VbMetaImageVerificationError::SignatureMismatch)
//         }
//     }
// }

// /// Read the AVB footer, if present, given a reader that's positioned at the end of the image.
// fn read_avb_footer<R: Read + Seek>(image: &mut R) -> Result<AvbFooter, VbMetaImageParseError> {
//     image.seek(SeekFrom::Current(-(size_of::<AvbFooter>() as i64)))?;
//     let mut raw_footer = [0u8; size_of::<AvbFooter>()];
//     image.read_exact(&mut raw_footer)?;
//     // SAFETY: the slice is the same size as the struct which only contains simple data types.
//     let mut footer = unsafe { transmute::<[u8; size_of::<AvbFooter>()], AvbFooter>(raw_footer) };
//     // SAFETY: the function updates the struct in-place.
//     if unsafe { avb_footer_validate_and_byteswap(&footer, &mut footer) } {
//         Ok(footer)
//     } else {
//         Err(VbMetaImageParseError::InvalidFooter)
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result};
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
        let vbmeta = VbMetaImage::verify_path(test_file).context("verify_path")?;
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
        let vbmeta = VbMetaImage::verify_path(&test_file).context("verify_path")?;

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
