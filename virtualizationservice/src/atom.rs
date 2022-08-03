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

//! Functions for creating and collecting atoms.

use crate::aidl::clone_file;
use android_system_virtualizationservice::aidl::android::system::virtualizationservice::{
    IVirtualMachine::IVirtualMachine, VirtualMachineAppConfig::VirtualMachineAppConfig,
    VirtualMachineConfig::VirtualMachineConfig,
};
use android_system_virtualizationservice::binder::{ParcelFileDescriptor, Status, Strong};
use anyhow::Result;
use log::{trace, warn};
use microdroid_payload_config::VmPayloadConfig;
use statslog_virtualization_rust::vm_creation_requested::{
    ConfigType, Hypervisor, VmCreationRequested,
};
use std::fs::read_link;
use std::os::unix::io::AsRawFd;
use zip::ZipArchive;

fn get_vm_payload_config(config: &VirtualMachineAppConfig) -> Result<VmPayloadConfig> {
    let apk_file = clone_file(config.apk.as_ref().unwrap())?;
    let mut apk_zip = ZipArchive::new(&apk_file)?;
    let config_file = apk_zip.by_name(&config.configPath)?;
    let vm_payload_config: VmPayloadConfig = serde_json::from_reader(config_file)?;
    Ok(vm_payload_config)
}

fn get_main_apk_path(fd: &ParcelFileDescriptor) -> Result<String> {
    let raw_fd = fd.as_raw_fd();
    let link = read_link(format!("/proc/self/fd/{}", raw_fd)).unwrap();
    let path = link.into_os_string().into_string().unwrap();
    Ok(path)
}

/// Write the stats of VMCreation to statsd
pub fn write_vm_creation_stats(
    config: &VirtualMachineConfig,
    is_protected: bool,
    ret: &binder::Result<Strong<dyn IVirtualMachine>>,
) {
    let creation_succeeded;
    let binder_exception_code;
    match ret {
        Ok(_) => {
            creation_succeeded = true;
            binder_exception_code = Status::ok().exception_code() as i32;
        }
        Err(ref e) => {
            creation_succeeded = false;
            binder_exception_code = e.exception_code() as i32;
        }
    }

    let empty_string = String::new();
    let mut main_apk = String::new();
    let mut extra_apks = String::new();
    let mut apexes = String::new();
    let vm_creation_requested = match config {
        VirtualMachineConfig::AppConfig(config) => {
            let vm_payload_config = get_vm_payload_config(config);
            VmCreationRequested {
                vm_name: &config.name,
                hypervisor: Hypervisor::Pkvm,
                is_protected,
                creation_succeeded,
                binder_exception_code,
                config_type: ConfigType::VirtualMachineAppConfig,
                num_cpus: config.numCpus,
                cpu_affinity: config.cpuAffinity.as_ref().unwrap_or(&empty_string),
                memory_mib: config.memoryMib,
                main_apk: {
                    if config.apk.is_some() {
                        main_apk = get_main_apk_path(config.apk.as_ref().unwrap())
                            .as_ref()
                            .unwrap_or(&empty_string)
                            .to_string();
                    }
                    &main_apk
                },
                extra_apks: {
                    if vm_payload_config.is_ok() {
                        for extra_apk_config in &vm_payload_config.as_ref().unwrap().extra_apks {
                            extra_apks.push_str(&extra_apk_config.path);
                            extra_apks.push(':');
                        }
                    }
                    &extra_apks
                },
                apexes: {
                    if vm_payload_config.is_ok() {
                        for apexes_config in &vm_payload_config.as_ref().unwrap().apexes {
                            apexes.push_str(&apexes_config.name);
                            apexes.push(':');
                        }
                    }
                    &apexes
                },
                // TODO(seungjaeyoo) Fill information about task_profile
            }
        }
        VirtualMachineConfig::RawConfig(config) => VmCreationRequested {
            vm_name: &config.name,
            hypervisor: Hypervisor::Pkvm,
            is_protected,
            creation_succeeded,
            binder_exception_code,
            config_type: ConfigType::VirtualMachineRawConfig,
            num_cpus: config.numCpus,
            cpu_affinity: config.cpuAffinity.as_ref().unwrap_or(&empty_string),
            memory_mib: config.memoryMib,
            main_apk: &empty_string,
            extra_apks: &empty_string,
            apexes: &empty_string,
            // TODO(seungjaeyoo) Fill information about task_profile
            // TODO(seungjaeyoo) Fill information about disk_image for raw config
        },
    };
    match vm_creation_requested.stats_write() {
        Err(e) => {
            warn!("statslog_rust failed with error: {}", e);
        }
        Ok(_) => trace!("statslog_rust succeeded for virtualization service"),
    }
}
