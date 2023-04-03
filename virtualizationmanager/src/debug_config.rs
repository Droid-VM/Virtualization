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
use core::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::io::Read;
use log::info;
use rustutils::system_properties;
use libfdt::Fdt;

const DEBUG_POLICY_FILE_PREFIX: &str = "/proc/device-tree";
const DEBUG_POLICY_LOG_PATH: &str = "/avf/guest/common/log";
const DEBUG_POLICY_RAMDUMP_PATH: &str = "/avf/guest/common/ramdump";
const DEBUG_POLICY_ADB_PATH: &str = "/avf/guest/microdroid/adb";

const SYSPROP_CUSTOM_DEBUG_POLICY_PATH: &str = "hypervisor.virtualizationmanager.debug_policy.path";
const MAX_CUSTOM_DEBUG_POLICY_SIZE_BYTES: usize = 500; // Note: Size of DTB with everything <300.

/// Get debug policy value in bool. It's true iff the value is explicitly set to <1>.
fn get_debug_policy_bool(path: &'static str) -> Option<bool> {
    let mut file = File::open(DEBUG_POLICY_FILE_PREFIX.to_owned() + path).ok()?;
    let mut log: [u8; 4] = Default::default();
    file.read_exact(&mut log).ok()?;
    // DT spec uses big endian although Android is always little endian.
    Some(u32::from_be_bytes(log) == 1)
}

fn read_file(path: &str, max_size: usize) -> Option<Vec<u8>> {
    let mut file = OpenOptions::new().read(true).open(path).ok()?;

    let mut buf = vec![0u8; max_size];

    // Return read result only when success to read end of the file.
    match file.read(buf.as_mut_slice()) {
        Ok(size) if size < max_size => Some(buf),
        _ => None   // Failed to fully read file.
    }
}

/// Fdt wrapper with array backed buffer.
struct FdtWrapper {
    buffer: [u8; MAX_CUSTOM_DEBUG_POLICY_SIZE_BYTES],
}

impl FdtWrapper {
    fn from_overlay_onto_new_fdt(overlay_file_path: &str) -> Option<Self> {
        let mut fdt_buf = [0u8; MAX_CUSTOM_DEBUG_POLICY_SIZE_BYTES];
        let fdt = Fdt::create_empty_tree(&mut fdt_buf).ok()?;

        let mut overlay_buf = read_file(overlay_file_path, MAX_CUSTOM_DEBUG_POLICY_SIZE_BYTES)?;
        let fdt_overlay = Fdt::from_mut_slice(overlay_buf.as_mut_slice()).ok()?;

        // SAFETY - We'll not return FdtWrapper if error happen.
        unsafe {
            fdt.apply_overlay(fdt_overlay).ok()?;
        }

        Some(Self {
            buffer: fdt_buf,
        })
    }

    fn as_fdt(&self) -> &Fdt {
        // SAFETY - We don't return FdtWrapper if damaged.
        unsafe { Fdt::unchecked_from_slice(&self.buffer) }
    }

    /// Get property value in bool. It's true iff the value is explicitly set to <1>.
    fn get_fdt_prop_bool(&self, path: &'static str) -> Option<bool> {
        let index = path.rfind('/')?;

        let node_path = path[0..index].to_owned() + "\0";
        let node_cstr = CStr::from_bytes_with_nul(node_path.as_bytes()).unwrap();
        let node = self.as_fdt().node(node_cstr).ok()??;

        let prop_name = path[index+1..path.len()].to_owned() + "\0";
        let prop_cstr = CStr::from_bytes_with_nul(prop_name.as_bytes()).unwrap();
        let prop = node.getprop_u32(prop_cstr).ok()??;

        Some(prop == 1)
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
        match system_properties::read(SYSPROP_CUSTOM_DEBUG_POLICY_PATH).unwrap_or_default() {
            Some(debug_policy_path) if !debug_policy_path.is_empty() => {
                if let Some(fdt) = FdtWrapper::from_overlay_onto_new_fdt(&debug_policy_path) {
                    // TODO: Remove this code path in user build.
                    let dp = Self {
                        debug_level,
                        debug_policy_log: fdt.get_fdt_prop_bool(DEBUG_POLICY_LOG_PATH)
                            .unwrap_or_default(),
                        debug_policy_ramdump: fdt.get_fdt_prop_bool(DEBUG_POLICY_RAMDUMP_PATH)
                            .unwrap_or_default(),
                        debug_policy_adb: fdt.get_fdt_prop_bool(DEBUG_POLICY_ADB_PATH)
                            .unwrap_or_default(),
                    }
                    info!("Loaded custom debug policy: {:?}", dp);

                    dp
                } else {
                    info!("Failed to apply provided overlay. Debug policies will be disabled");
                    Self {
                        debug_level,
                        debug_policy_log: false,
                        debug_policy_ramdump: false,
                        debug_policy_adb: false,
                    }
                }
            },
            _ => {
                let debug_config = Self {
                    debug_level,
                    debug_policy_log: get_debug_policy_bool(DEBUG_POLICY_LOG_PATH)
                        .unwrap_or_default(),
                    debug_policy_ramdump: get_debug_policy_bool(DEBUG_POLICY_RAMDUMP_PATH)
                        .unwrap_or_default(),
                    debug_policy_adb: get_debug_policy_bool(DEBUG_POLICY_ADB_PATH)
                        .unwrap_or_default(),
                };
                info!("Loaded debug policy from host OS: {:?}", debug_config);

                debug_config
            }
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
}
