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
        CpuTopology::CpuTopology, DiskImage::DiskImage, Partition::Partition,
        PartitionType::PartitionType, VirtualMachineConfig::VirtualMachineConfig,
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

const RIALTO_PATH: &str = "/apex/com.android.virt/etc/rialto.bin";

pub(crate) fn get_certificate(
    csr: &[u8],
    instance_img_fd: &ParcelFileDescriptor,
) -> Result<Vec<u8>> {
    // We need to start the thread pool for Binder to work properly, especially link_to_death.
    ProcessState::start_thread_pool();

    let virtmgr = vmclient::VirtualizationService::new()
        .map_err(|e| anyhow!(format!("Failed to spawn VirtualizationService. Error:{:?}", e)))?;
    let service = virtmgr.connect().map_err(|e| {
        anyhow!(format!("Failed to connect to VirtualizationService. Error:{:?}", e))
    })?;
    info!("rkpvm: Connected to VirtualizationService");
    let rialto = File::open(RIALTO_PATH).context("Failed to open Rialto kernel binary")?;
    let console = android_log_fd()?;
    let log = android_log_fd()?;

    info!("rkpvm: Preparing instance.img...");
    const INSTANCE_IMG_SIZE_10MB: i64 = 10 * 1024 * 1024;
    service
        .initializeWritablePartition(
            instance_img_fd,
            INSTANCE_IMG_SIZE_10MB,
            PartitionType::ANDROID_VM_INSTANCE,
        )
        .map_err(|e| anyhow!(format!("Failed to initialize instange.img: {:?}", e)))?;
    let instance_img = instance_img_fd
        .as_ref()
        .try_clone()
        .map_err(|e| anyhow!(format!("Failed to clone rkpvm_instance.img. Error:{:?}", e)))?;
    let instance_img = ParcelFileDescriptor::new(instance_img);
    let writable_partitions = vec![Partition {
        label: "vm-instance".to_owned(),
        image: Some(instance_img),
        writable: true,
    }];
    info!("rkpvm: Finished initializing instance.img...");

    let config = VirtualMachineConfig::RawConfig(VirtualMachineRawConfig {
        name: String::from("RKP VM"),
        kernel: None,
        initrd: None,
        params: None,
        bootloader: Some(ParcelFileDescriptor::new(rialto)),
        disks: vec![DiskImage { image: None, partitions: writable_partitions, writable: true }],
        protectedVm: true,
        memoryMib: 300,
        cpuTopology: CpuTopology::ONE_CPU,
        platformVersion: "~1.0".to_string(),
        taskProfiles: vec![],
        gdbPort: 0, // No gdb
    });
    let vm = VmInstance::create(service.as_ref(), &config, Some(console), Some(log), None)
        .context("Failed to create VM")?;

    info!("rkpvm: Starting RKP VM...");
    vm.start().context("Failed to start VM")?;

    // Wait for VM to finish.
    vm.wait_for_death_with_timeout(Duration::from_secs(10))
        .ok_or_else(|| anyhow!("Timed out waiting for VM exit"))?;

    info!("rkpvm: Finished getting the certificate");
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
