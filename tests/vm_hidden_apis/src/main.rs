// Copyright 2024, The Android Open Source Project
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

//! Integration test for VM internal APIs.

use android_system_virtualizationservice::{
    aidl::android::system::virtualizationservice::{
        CpuTopology::CpuTopology, DiskImage::DiskImage, VirtualMachineConfig::VirtualMachineConfig,
        VirtualMachineRawConfig::VirtualMachineRawConfig,
    },
    binder::{ParcelFileDescriptor, ProcessState},
};
use anyhow::{Context, Error};
use log::info;
use std::{fs::File, io::Write, panic};
use vmclient::{DeathReason, VmInstance};

const VMBASE_EXAMPLE_KERNEL_PATH: &str = "vmbase_example_kernel.bin";
const TEST_DISK_IMAGE_PATH: &str = "test_disk.img";
const EMPTY_DISK_IMAGE_PATH: &str = "empty_disk.img";

/// Runs the vm_hidden_apis VM as an unprotected VM via VirtualizationService and snapshots it.
#[test]
fn snapshot_test() -> Result<(), Error> {
    let kernel = Some(open_payload(VMBASE_EXAMPLE_KERNEL_PATH)?);
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("vm_hidden_apis")
            .with_max_level(log::LevelFilter::Debug),
    );

    // Redirect panic messages to logcat.
    panic::set_hook(Box::new(|panic_info| {
        log::error!("{}", panic_info);
    }));

    // We need to start the thread pool for Binder to work properly, especially link_to_death.
    ProcessState::start_thread_pool();

    let virtmgr =
        vmclient::VirtualizationService::new().context("Failed to spawn VirtualizationService")?;
    let service = virtmgr.connect().context("Failed to connect to VirtualizationService")?;

    // Make file for test disk image.
    let mut test_image = File::options()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(TEST_DISK_IMAGE_PATH)
        .with_context(|| format!("Failed to open test disk image {}", TEST_DISK_IMAGE_PATH))?;
    // Write 4 sectors worth of 4-byte numbers counting up.
    for i in 0u32..512 {
        test_image.write_all(&i.to_le_bytes())?;
    }
    let test_image = ParcelFileDescriptor::new(test_image);
    let disk_image = DiskImage { image: Some(test_image), writable: false, partitions: vec![] };

    // Make file for empty test disk image.
    let empty_image = File::options()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(EMPTY_DISK_IMAGE_PATH)
        .with_context(|| format!("Failed to open empty disk image {}", EMPTY_DISK_IMAGE_PATH))?;
    let empty_image = ParcelFileDescriptor::new(empty_image);
    let empty_disk_image =
        DiskImage { image: Some(empty_image), writable: false, partitions: vec![] };

    let config = VirtualMachineConfig::RawConfig(VirtualMachineRawConfig {
        name: String::from("VmBaseTest"),
        kernel,
        initrd: None,
        params: None,
        bootloader: None,
        disks: vec![disk_image, empty_disk_image],
        protectedVm: false,
        memoryMib: 300,
        cpuTopology: CpuTopology::ONE_CPU,
        platformVersion: "~1.0".to_string(),
        gdbPort: 0, // no gdb
        ..Default::default()
    });
    let vm =
        VmInstance::create(service.as_ref(), &config, None, /* consoleIn */ None, None, None)
            .context("Failed to create VM")?;
    vm.start().context("Failed to start VM")?;
    info!("Started example VM.");
    std::fs::create_dir_all("snapshot")?;
    let snapdir =
        ParcelFileDescriptor::new(File::open("snapshot").context("failed to open snapshot dir")?);

    vm.vm.snapshot(&snapdir, false).context("failed to snapshot")?;

    std::fs::remove_dir_all("snapshot")?;
    // Wait for VM to finish, and check that it shut down cleanly.
    let death_reason = vm.wait_for_death();

    assert_eq!(death_reason, DeathReason::Shutdown);

    Ok(())
}

fn open_payload(path: &str) -> Result<ParcelFileDescriptor, Error> {
    let file = File::open(path).with_context(|| format!("Failed to open VM image {path}"))?;
    Ok(ParcelFileDescriptor::new(file))
}
