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

//! Integration test for Portal kernel.

use android_system_virtualizationservice::{
    aidl::android::system::virtualizationservice::{
        VirtualMachineConfig::VirtualMachineConfig,
        VirtualMachineRawConfig::VirtualMachineRawConfig,
    },
    binder::{ParcelFileDescriptor, ProcessState},
};
use anyhow::{Context, Error};
use log::info;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::os::unix::io::FromRawFd;
use std::panic;
use std::thread;
use vmclient::{DeathReason, VmInstance};

const PORTAL_PATH: &str = "/data/local/tmp/portal_test/arm64/portal.bin";

/// Runs a portal VM as a non-protected VM via VirtualizationService.
#[test]
fn test_run_portal() -> Result<(), Error> {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("portal")
            .with_min_level(log::Level::Debug),
    );
    // Redirect panic messages to logcat.
    panic::set_hook(Box::new(|panic_info| {
        log::error!("{}", panic_info);
    }));

    // We need to start the thread pool for Binder to work properly, especially link_to_death.
    ProcessState::start_thread_pool();

    let service = vmclient::connect().context("Failed to find VirtualizationService")?;

    let portal =File::open(PORTAL_PATH)
        .context(format!("Failed to open {}", PORTAL_PATH))?;

    let console = logging_fd()?;
    let log = logging_fd()?;

    let config = VirtualMachineConfig::RawConfig(VirtualMachineRawConfig {
        kernel: None,
        initrd: None,
        params: None,
        bootloader: Some(ParcelFileDescriptor::new(portal)),
        disks: vec![],
        protectedVm: false,
        memoryMib: 300,
        numCpus: 1,
        cpuAffinity: None,
        platformVersion: "~1.0".to_string(),
        taskProfiles: vec![],
    });
    let vm = VmInstance::create(service.as_ref(), &config, Some(console), Some(log))
        .context("Failed to create VM")?;

    vm.start().context("Failed to start VM")?;
    info!("Started example VM.");

    // Wait for VM to finish, and check that it shut down cleanly.
    let death_reason = vm.wait_for_death();
    assert_eq!(death_reason, DeathReason::Shutdown);

    Ok(())
}

fn create_pipe() -> io::Result<(File, File)> {
    let mut fds = [0; 2];
    let res = unsafe { libc::pipe(&mut fds as *mut libc::c_int) };
    if res != 0 {
        return Err(io::Error::last_os_error());
    }

    let reader = unsafe { File::from_raw_fd(fds[0]) };
    let writer = unsafe { File::from_raw_fd(fds[1]) };
    Ok((reader, writer))
}

fn logging_fd() -> io::Result<File> {
    let (reader, writer) = create_pipe()?;

    thread::spawn(|| {
        for line in BufReader::new(reader).lines() {
            info!("{}", line.unwrap());
        }
    });
    Ok(writer)
}
