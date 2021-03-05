//! Function and types for VM configuration.

use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;

/// Configuration for a particular VM to be started.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VmConfig {
    /// The filename of the kernel image, if any.
    pub kernel: Option<String>,
    /// The filename of the initial ramdisk for the kernel, if any.
    pub initrd: Option<String>,
    /// Parameters to pass to the kernel.
    pub params: Option<String>,
    /// The bootloader to use. If this is supplied then the kernel and initrd must not be supplied;
    /// the bootloader is instead responsibly for loading the kernel from one of the disks.
    pub bootloader: Option<String>,
    /// Disk images to be made available to the VM.
    #[serde(default)]
    pub disks: Vec<DiskImage>,
}

/// A disk image to be made available to the VM.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskImage {
    /// The filename of the disk image.
    pub image: String,
    /// Whether this disk should be writable by the VM.
    pub writable: bool,
}

/// Load the configuration for the VM with the given ID from a JSON file.
pub fn load_vm_config(path: &str) -> Result<VmConfig, Error> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path))?;
    let buffered = BufReader::new(file);
    Ok(serde_json::from_reader(buffered)?)
}
