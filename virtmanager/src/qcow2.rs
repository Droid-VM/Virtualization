// Copyright 2021, The Android Open Source Project
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

use anyhow::{anyhow, Context, Error};
use disk::QcowFile;
use log::info;
use std::fs::{File, OpenOptions};
use std::os::unix::prelude::AsRawFd;
use std::path::Path;

/// Construct a QCOW2 image to wrap the given backing disk image.
///
/// Returns the QCOW2 image file.
pub fn make_qcow2_image(
    backing_file: &File,
    output_filename: &Path,
    writable: bool,
) -> Result<File, Error> {
    info!("Making QCOW2 image {:?}", output_filename);
    let output_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(output_filename)
        .with_context(|| format!("Failed to create QCOW2 image {:?}", output_filename))?;
    let backing_filename = format!("/proc/self/fd/{}", backing_file.as_raw_fd());
    QcowFile::new_from_backing(output_file, &backing_filename)
        .map_err(|e| anyhow!("Failed to create QCOW2 image {:?}: {:?}", output_filename, e))?;

    // new_from_backing consumes output_file, so we need to open it again to return.
    let output_file = OpenOptions::new()
        .read(true)
        .write(writable)
        .open(output_filename)
        .with_context(|| format!("Failed to open QCOW2 image {:?}", output_filename))?;
    Ok(output_file)
}
