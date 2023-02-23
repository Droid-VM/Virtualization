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

//! Handles the RKP VM and host communication.

use android_system_virtualizationservice::{
    aidl::android::system::virtualizationservice::{
        CpuTopology::CpuTopology, VirtualMachineConfig::VirtualMachineConfig,
        VirtualMachineRawConfig::VirtualMachineRawConfig,
    },
    binder::{ParcelFileDescriptor, ProcessState},
};
use anyhow::{anyhow, Context, Result};
use log::info;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::os::unix::io::FromRawFd;
use std::thread;
use std::time::Duration;
use vmclient::VmInstance;

const RIALTO_PATH: &str = "/apex/com.android.virt/bin/rialto.bin";

pub(crate) fn generate_certificate(csr: &[u8]) -> Result<Vec<u8>> {
    // We need to start the thread pool for Binder to work properly, especially link_to_death.
    ProcessState::start_thread_pool();

    let service = vmclient::connect().map_err(|e| {
        anyhow!(format!("Failed to connect to VirtualizationService. Error: '{:?}'", e))
    })?;
    info!("rkpvm: Connected to VirtualizationService");
    let rialto = File::open(RIALTO_PATH).context("Failed to open Rialto kernel binary")?;
    let console = android_log_fd()?;
    let log = android_log_fd()?;

    let config = VirtualMachineConfig::RawConfig(VirtualMachineRawConfig {
        name: String::from("RKP VM"),
        kernel: None,
        initrd: None,
        params: None,
        bootloader: Some(ParcelFileDescriptor::new(rialto)),
        disks: vec![],
        protectedVm: true,
        memoryMib: 300,
        cpuTopology: CpuTopology::ONE_CPU,
        platformVersion: "~1.0".to_string(),
        taskProfiles: vec![],
        gdbPort: 0, // No gdb
    });
    let vm = VmInstance::create(service.as_ref(), &config, Some(console), Some(log), None)
        .context("Failed to create VM")?;

    vm.start().context("Failed to start VM")?;

    // Wait for VM to finish.
    vm.wait_for_death_with_timeout(Duration::from_secs(10))
        .ok_or_else(|| anyhow!("Timed out waiting for VM exit"))?;

    Ok([b"Return: ", csr].concat())
}

fn android_log_fd() -> io::Result<File> {
    let (reader_fd, writer_fd) = nix::unistd::pipe()?;

    // SAFETY: These are new FDs with no previous owner.
    let reader = unsafe { File::from_raw_fd(reader_fd) };
    let writer = unsafe { File::from_raw_fd(writer_fd) };

    thread::spawn(|| {
        for line in BufReader::new(reader).lines() {
            info!("{}", line.unwrap());
        }
    });
    Ok(writer)
}
