//! Function and types for VM configuration.

use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;

/// Configuration for a particular VM to be started.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VmConfig {
    pub kernel: Option<String>,
    pub initrd: Option<String>,
    pub params: Option<String>,
    pub bootloader: Option<String>,
    #[serde(default)]
    pub disks: Vec<DiskImage>,
}

/// A disk image to be made available to the VM.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskImage {
    pub image: String,
    pub writable: bool,
}

/// Load the configuration for the VM with the given ID from a JSON file.
pub fn load_vm_config(path: &str) -> Result<VmConfig, Error> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path))?;
    let buffered = BufReader::new(file);
    Ok(serde_json::from_reader(buffered)?)
}
