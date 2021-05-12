use anyhow::{anyhow, Context, Error};
use disk::QcowFile;
use log::info;
use std::fs::{File, OpenOptions};
use std::os::unix::prelude::AsRawFd;
use std::path::Path;

/// Construct a QCOW2 image to wrap the given backing disk image.
///
/// Returns the QCOW2 image file, and a FD mapping which must be applied to any process which wants
/// to use it. This is necessary because the image contains a path of the form `/proc/self/fd/N` for
/// the backing image.
pub fn make_qcow2_image(backing_file: &File, output_filename: &Path) -> Result<File, Error> {
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
    let output_file = File::open(output_filename)
        .with_context(|| format!("Failed to open QCOW2 image {:?}", output_filename))?;
    Ok(output_file)
}
