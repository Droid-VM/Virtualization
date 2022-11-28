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

use avb_bindgen::AvbFooter;
use avb_nostd::validate_avb_footer;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::mem::{size_of, MaybeUninit};
use std::path::Path;
use std::slice;

pub use avb_nostd::{Descriptor, VbMetaImage, VbMetaImageParseError, VbMetaImageVerificationError};

/// Load and verify a VBMeta image from the given path.
pub fn verify_vbmeta_from_path<P: AsRef<Path>>(
    path: P,
) -> Result<VbMetaImage, VbMetaImageVerificationError> {
    let file =
        File::open(path).map_err(|e| VbMetaImageVerificationError::Io(format!("{:?}", e)))?;
    let size =
        file.metadata().map_err(|e| VbMetaImageVerificationError::Io(format!("{:?}", e)))?.len();
    verify_vbmeta_from_reader(file, 0, size)
}

/// Load and verify a VBMeta image from a region within a reader.
pub fn verify_vbmeta_from_reader<R: Read + Seek>(
    mut image: R,
    offset: u64,
    size: u64,
) -> Result<VbMetaImage, VbMetaImageVerificationError> {
    // Check for a footer in the image or assume it's an entire VBMeta image.
    image
        .seek(SeekFrom::Start(offset + size))
        .map_err(|e| VbMetaImageVerificationError::Io(format!("{:?}", e)))?;
    let footer = read_avb_footer(&mut image)
        .map_err(|e| VbMetaImageVerificationError::Io(format!("{:?}", e)))?;
    let (vbmeta_offset, vbmeta_size) = if let Some(footer) = footer {
        if footer.vbmeta_offset > size || footer.vbmeta_size > size - footer.vbmeta_offset {
            return Err(VbMetaImageVerificationError::InvalidFooter);
        }
        (footer.vbmeta_offset, footer.vbmeta_size)
    } else {
        (0, size)
    };
    image
        .seek(SeekFrom::Start(offset + vbmeta_offset))
        .map_err(|e| VbMetaImageVerificationError::Io(format!("{:?}", e)))?;
    // Verify the image before examining it to check the size.
    let mut data = vec![0u8; vbmeta_size as usize];
    image
        .read_exact(&mut data)
        .map_err(|e| VbMetaImageVerificationError::Io(format!("{:?}", e)))?;
    VbMetaImage::verify_vbmeta(&data)
}

/// Reads the AVB footer, if present, given a reader that's positioned at the end of the image.
fn read_avb_footer<R: Read + Seek>(image: &mut R) -> io::Result<Option<AvbFooter>> {
    image.seek(SeekFrom::Current(-(size_of::<AvbFooter>() as i64)))?;
    // SAFETY: the slice is the same size as the struct which only contains simple data types.
    let footer = unsafe {
        let mut footer = MaybeUninit::<AvbFooter>::uninit();
        let footer_slice =
            slice::from_raw_parts_mut(&mut footer as *mut _ as *mut u8, size_of::<AvbFooter>());
        image.read_exact(footer_slice)?;
        footer.assume_init()
    };
    Ok(validate_avb_footer(footer))
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
    fn test_unsigned_image() -> Result<()> {
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
        let vbmeta =
            verify_vbmeta_from_path(test_file).map_err(|e| anyhow!("verify path failed: {}", e))?;
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
        let vbmeta = verify_vbmeta_from_path(&test_file)
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
            verify_vbmeta_from_path(test_file),
            Err(VbMetaImageVerificationError::HashMismatch)
        ));
        Ok(())
    }

    #[test]
    fn test_rsa2048_signed_image() -> Result<()> {
        signed_image_has_valid_vbmeta("SHA256_RSA2048", "testdata/testkey_rsa2048.pem")
    }

    #[test]
    fn test_rsa4096_signed_image() -> Result<()> {
        signed_image_has_valid_vbmeta("SHA256_RSA4096", "testdata/testkey_rsa4096.pem")
    }

    #[test]
    fn test_rsa8192_signed_image() -> Result<()> {
        signed_image_has_valid_vbmeta("SHA256_RSA8192", "testdata/testkey_rsa8192.pem")
    }
}
