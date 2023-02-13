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

//! Android Debug Policy for AVF

use android_system_virtualizationservice::aidl::android::system::virtualizationservice::{
    VirtualMachineAppConfig::DebugLevel::DebugLevel
};
use std::fs::File;
use std::io::Read;

fn get_debug_policy_bool(path: &'static str) -> Option<bool> {
    if let Ok(mut file) = File::open(path) {
        let mut log: [u8; 4] = Default::default();
        file.read_exact(&mut log).map_err(|_| false).unwrap();
        // DT spec uses big endian although Android is always little endian.
        return Some(u32::from_be_bytes(log) == 1);
    }
    None
}

/// Return whether debug apexes (MICRODROID_REQUIRED_APEXES_DEBUG) are required.
/// Adb may not be enabled here,
/// Return whether microdroid's adb is allowed and required to be configured.
/// adb will be enabled later in the microdroid, and we only need to configure here.
pub fn should_include_debug_apexes(debug_level: DebugLevel) -> bool {
    debug_level != DebugLevel::NONE
        || get_debug_policy_bool("/proc/device-tree/avf/guest/microdroid/adb").unwrap_or_default()
}

/// Return whether console output should be configred for VM for adb log.
/// Caller should create pipe and prepare for receiving VM log with it.
pub fn should_prepare_console_output(debug_level: DebugLevel) -> bool {
    debug_level != DebugLevel::NONE
        || get_debug_policy_bool("/proc/device-tree/avf/guest/common/adb").unwrap_or_default()
}
