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

//! Functions for AVF debug policy and debug level

use android_system_virtualizationservice::aidl::android::system::virtualizationservice::{
    VirtualMachineAppConfig::DebugLevel::DebugLevel,
};
use anyhow::{anyhow, ensure, Context, Error, Result};
use core::ffi::CStr;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read};
use std::path::Path;
use log::{warn, info};
use rustutils::system_properties;
use libfdt::{Fdt, FdtError};

macro_rules! cstr {
    ($str:literal) => {{
        // SAFETY -- must only be used for const expaction.
        unsafe {
            // CStr::from_bytes_with_nul() is currently 'const: unstable',
            // so picked `from_bytes_with_nul_unchecked()` although it's unsafe.
            // Maybe OK because we would only use it for literals.
            CStr::from_bytes_with_nul_unchecked($str)
        }
    }};
}

const DEBUG_POLICY_LOG_PATH: &str = "/sys/firmware/devicetree/base/avf/guest/common/log";
const DEBUG_POLICY_RAMDUMP_PATH: &str = "/sys/firmware/devicetree/base/avf/guest/common/ramdump";
const DEBUG_POLICY_ADB_PATH: &str = "/sys/firmware/devicetree/base/avf/guest/microdroid/adb";

const DEBUG_POLICY_LOG_DT_NODE_PROP: (&CStr, &CStr) =
    (cstr!(b"/avf/guest/common\0"), cstr!(b"log\0"));
const DEBUG_POLICY_RAMDUMP_DT_NODE_PROP: (&CStr, &CStr) =
    (cstr!(b"/avf/guest/common\0"), cstr!(b"ramdump\0"));
const DEBUG_POLICY_ADB_DT_NODE_PROP: (&CStr, &CStr) =
    (cstr!(b"/avf/guest/microdroid\0"), cstr!(b"adb\0"));

const CUSTOM_DEBUG_POLICY_OVERLAY_SYSPROP: &str =
    "hypervisor.virtualizationmanager.debug_policy.path";
const DEVICE_TREE_EMPTY_TREE_SIZE_BYTES: usize = 100; // rough estimation.

/// Get debug policy value in bool. It's true iff the value is explicitly set to <1>.
fn get_debug_policy_bool(path: &Path) -> Result<Option<bool>> {
    let mut file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => Err(error).with_context(|| format!("Failed to open {path:?}"))?,
    };

    let mut value = vec![0_u8, 4];
    let file_size =
        file.read_to_end(&mut value).with_context(|| format!("Failed to read {:?}", path))?;
    ensure!(
        file_size == 4 && value.len() == 4,
        format!(
            "Malformed data in {path:?}. Must be 32 bytes for bool {file_size} {}",
            value.len()
        )
    );

    // DT spec uses big endian although Android is always little endian.
    match u32::from_be_bytes(value.try_into().unwrap()) {
        0 => Ok(Some(false)),
        1 => Ok(Some(true)),
        value => Err(anyhow!(
            "Invalid value in {path:?}. Expected <0> or <1> for bool, but was {value}."
        )),
    }
}

/// Fdt wrapper with array backed buffer.
struct FdtWrapper {
    buffer: Vec<u8>,
}

impl FdtWrapper {
    fn from_overlay_onto_new_fdt(overlay_file_path: &Path) -> Result<Option<Self>> {
        let mut overlay_buf = Vec::<u8>::new();
        let mut overlay_file = match OpenOptions::new().read(true).open(overlay_file_path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                Err(error).with_context(|| format!("Failed to open {overlay_file_path:?}"))?
            }
        };
        let overlay_size = overlay_file
            .read_to_end(&mut overlay_buf)
            .with_context(|| "Failed to read {overlay_file_path:?}")?;
        let fdt_overlay = Fdt::from_mut_slice(overlay_buf.as_mut_slice())
            .map_err(Error::msg)
            .with_context(|| "Malformed {overlay_file_path:?}")?;

        let fdt_estimated_size = overlay_size + DEVICE_TREE_EMPTY_TREE_SIZE_BYTES;
        let mut fdt_buf = vec![0_u8; fdt_estimated_size];
        let fdt = Fdt::create_empty_tree(fdt_buf.as_mut_slice())
            .map_err(Error::msg)
            .context("Failed to create an empty device tree")?;

        // SAFETY - Return immediately if error happen, and also discard damaged fdt_buf and fdt.
        unsafe {
            fdt.apply_overlay(fdt_overlay).map_err(Error::msg).with_context(|| {
                "Failed to overlay {overlay_file_path:?} onto empty device tree"
            })?;
        }

        Ok(Some(Self { buffer: fdt_buf }))
    }

    fn as_fdt_unchecked(&self) -> &Fdt {
        // SAFETY - Checked validity of buffer when instantiate.
        unsafe { Fdt::unchecked_from_slice(&self.buffer) }
    }

    /// Get property value in bool. It's true iff the value is explicitly set to <1>.
    fn get_fdt_prop_bool(&self, path: (&CStr, &CStr)) -> Result<Option<bool>> {
        let (node_path, prop_name) = path;

        let node = match self.as_fdt_unchecked().node(node_path) {
            Ok(Some(node)) => node,
            Err(error) if error != FdtError::NotFound => Err(error)
                .map_err(Error::msg)
                .with_context(|| format!("Failed to get node {node_path:?}"))?,
            _ => return Ok(None),
        };

        match node.getprop_u32(prop_name) {
            Ok(Some(0)) => Ok(Some(false)),
            Ok(Some(1)) => Ok(Some(true)),
            Ok(Some(value)) => Err(anyhow!("Invalid prop value {prop_name:?} in node {node_path:?}. Expected <0> or <1> for bool, but was {value}")),
            Err(error) if error != FdtError::NotFound => Err(error).map_err(Error::msg).with_context(|| format!("Failed to get prop {prop_name:?}")),
            _ => Ok(None),
        }
    }
}

/// Debug configurations for both debug level and debug policy
#[derive(Debug)]
pub struct DebugConfig {
    pub debug_level: DebugLevel,
    debug_policy_log: bool,
    debug_policy_ramdump: bool,
    debug_policy_adb: bool,
}

impl DebugConfig {
    pub fn new(debug_level: DebugLevel) -> Self {
        match system_properties::read(CUSTOM_DEBUG_POLICY_OVERLAY_SYSPROP).unwrap_or_default() {
            Some(path) if !path.is_empty() => {
                match Self::from_custom_debug_overlay_policy(debug_level, Path::new(&path)) {
                    Ok(Some(debug_config)) => {
                        info!("Loaded custom debug policy overlay {path}: {debug_config:?}");
                        return debug_config;
                    }
                    Ok(None) => info!("Provided custom debug policy overlay {path} was empty"),
                    Err(err) => warn!("Failed to load custom debug policy overlay {path}: {err:?}"),
                };
            }
            _ => {
                match Self::from_host(debug_level) {
                    Ok(debug_config) => {
                        info!("Loaded debug policy from host OS: {debug_config:?}");
                        return debug_config;
                    }
                    Err(err) => warn!("Failed to load debug policy from host OS: {err:?}"),
                };
            }
        }

        info!("Debug policy is disabled");
        Self {
            debug_level,
            debug_policy_log: false,
            debug_policy_ramdump: false,
            debug_policy_adb: false,
        }
    }

    /// Get whether console output should be configred for VM to leave console and adb log.
    /// Caller should create pipe and prepare for receiving VM log with it.
    pub fn should_prepare_console_output(&self) -> bool {
        self.debug_level != DebugLevel::NONE || self.debug_policy_log || self.debug_policy_adb
    }

    /// Get whether debug apexes (MICRODROID_REQUIRED_APEXES_DEBUG) are required.
    pub fn should_include_debug_apexes(&self) -> bool {
        self.debug_level != DebugLevel::NONE || self.debug_policy_adb
    }

    /// Decision to support ramdump
    pub fn is_ramdump_needed(&self) -> bool {
        self.debug_level != DebugLevel::NONE || self.debug_policy_ramdump
    }

    // TODO: Remove this code path in user build for removing libfdt depenency.
    fn from_custom_debug_overlay_policy(
        debug_level: DebugLevel,
        path: &Path,
    ) -> Result<Option<Self>> {
        match FdtWrapper::from_overlay_onto_new_fdt(path) {
            Ok(Some(fdt)) => Ok(Some(Self {
                debug_level,
                debug_policy_log: fdt
                    .get_fdt_prop_bool(DEBUG_POLICY_LOG_DT_NODE_PROP)?
                    .unwrap_or_default(),
                debug_policy_ramdump: fdt
                    .get_fdt_prop_bool(DEBUG_POLICY_RAMDUMP_DT_NODE_PROP)?
                    .unwrap_or_default(),
                debug_policy_adb: fdt
                    .get_fdt_prop_bool(DEBUG_POLICY_ADB_DT_NODE_PROP)?
                    .unwrap_or_default(),
            })),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn from_host(debug_level: DebugLevel) -> Result<Self> {
        Ok(Self {
            debug_level,
            debug_policy_log: get_debug_policy_bool(DEBUG_POLICY_LOG_PATH.as_ref())?
                .unwrap_or_default(),
            debug_policy_ramdump: get_debug_policy_bool(DEBUG_POLICY_RAMDUMP_PATH.as_ref())?
                .unwrap_or_default(),
            debug_policy_adb: get_debug_policy_bool(DEBUG_POLICY_ADB_PATH.as_ref())?
                .unwrap_or_default(),
        })
    }
}
