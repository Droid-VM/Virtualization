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

//! Integration test for VM bootloader.

mod sync;

use crate::sync::AtomicFlag;
use android_system_virtualizationservice::{
    aidl::android::system::virtualizationservice::{
        DeathReason::DeathReason, IVirtualMachine::IVirtualMachine,
        IVirtualMachineCallback::BnVirtualMachineCallback,
        IVirtualMachineCallback::IVirtualMachineCallback,
        IVirtualizationService::IVirtualizationService, VirtualMachineConfig::VirtualMachineConfig,
        VirtualMachineRawConfig::VirtualMachineRawConfig,
    },
    binder::{
        wait_for_interface, BinderFeatures, DeathRecipient, IBinder, Interface,
        ParcelFileDescriptor, ProcessState, Result as BinderResult, Strong,
    },
};
use anyhow::{Context, Error};
use log::info;
use std::{
    fs::File,
    io::{self, BufRead, BufReader},
    os::unix::io::{AsRawFd, FromRawFd},
};

const VIRTUALIZATION_SERVICE_BINDER_SERVICE_IDENTIFIER: &str =
    "android.system.virtualizationservice";
const VMBASE_EXAMPLE_PATH: &str =
    "/data/local/tmp/vmbase_example.integration_test/arm64/vmbase_example.bin";

/// Runs the vmbase_example VM as an unprotected VM via VirtualizationService.
#[test]
fn test_run_example_vm() -> Result<(), Error> {
    env_logger::init();

    // We need to start the thread pool for Binder to work properly, especially link_to_death.
    ProcessState::start_thread_pool();

    let service: Strong<dyn IVirtualizationService> =
        wait_for_interface(VIRTUALIZATION_SERVICE_BINDER_SERVICE_IDENTIFIER)
            .context("Failed to find VirtualizationService")?;

    // Start example VM.
    let bootloader = ParcelFileDescriptor::new(
        File::open(VMBASE_EXAMPLE_PATH)
            .with_context(|| format!("Failed to open VM image {}", VMBASE_EXAMPLE_PATH))?,
    );
    let config = VirtualMachineConfig::RawConfig(VirtualMachineRawConfig {
        kernel: None,
        initrd: None,
        params: None,
        bootloader: Some(bootloader),
        disks: vec![],
        protectedVm: false,
        memoryMib: 300,
        numCpus: 1,
        cpuAffinity: None,
        platformVersion: "~1.0".to_string(),
        taskProfiles: vec![],
    });
    let console = ParcelFileDescriptor::new(duplicate_stdout()?);
    let log = ParcelFileDescriptor::new(duplicate_stdout()?);
    let vm =
        service.createVm(&config, Some(&console), Some(&log)).context("Failed to create VM")?;
    vm.start()?;
    info!("Started example VM.");

    // Wait for VM to finish, and check that it shut down cleanly.
    wait_for_vm(vm)?;

    Ok(())
}

/// Wait until the given VM or the VirtualizationService itself dies.
fn wait_for_vm(vm: Strong<dyn IVirtualMachine>) -> Result<(), Error> {
    let dead = AtomicFlag::default();
    let callback = BnVirtualMachineCallback::new_binder(
        VirtualMachineCallback { dead: dead.clone() },
        BinderFeatures::default(),
    );
    vm.registerCallback(&callback)?;
    let death_recipient = wait_for_death(&mut vm.as_binder(), dead.clone())?;
    dead.wait();
    // Ensure that death_recipient isn't dropped before we wait on the flag, as it is removed
    // from the Binder when it's dropped.
    drop(death_recipient);
    Ok(())
}

/// Raise the given flag when the given Binder object dies.
///
/// If the returned DeathRecipient is dropped then this will no longer do anything.
fn wait_for_death(binder: &mut impl IBinder, dead: AtomicFlag) -> Result<DeathRecipient, Error> {
    let mut death_recipient = DeathRecipient::new(move || {
        eprintln!("VirtualizationService unexpectedly died");
        dead.raise();
    });
    binder.link_to_death(&mut death_recipient)?;
    Ok(death_recipient)
}

#[derive(Debug)]
struct VirtualMachineCallback {
    dead: AtomicFlag,
}

impl Interface for VirtualMachineCallback {}

impl IVirtualMachineCallback for VirtualMachineCallback {
    fn onPayloadStarted(
        &self,
        _cid: i32,
        stream: Option<&ParcelFileDescriptor>,
    ) -> BinderResult<()> {
        // Show the output of the payload
        if let Some(stream) = stream {
            let mut reader = BufReader::new(stream.as_ref());
            loop {
                let mut s = String::new();
                match reader.read_line(&mut s) {
                    Ok(0) => break,
                    Ok(_) => print!("{}", s),
                    Err(e) => eprintln!("error reading from virtual machine: {}", e),
                };
            }
        }
        Ok(())
    }

    fn onPayloadReady(&self, _cid: i32) -> BinderResult<()> {
        eprintln!("payload is ready");
        Ok(())
    }

    fn onPayloadFinished(&self, _cid: i32, exit_code: i32) -> BinderResult<()> {
        eprintln!("payload finished with exit code {}", exit_code);
        Ok(())
    }

    fn onError(&self, _cid: i32, error_code: i32, message: &str) -> BinderResult<()> {
        eprintln!("VM encountered an error: code={}, message={}", error_code, message);
        Ok(())
    }

    fn onDied(&self, _cid: i32, reason: DeathReason) -> BinderResult<()> {
        self.dead.raise();

        match reason {
            DeathReason::INFRASTRUCTURE_ERROR => println!("Error waiting for VM to finish."),
            DeathReason::KILLED => println!("VM was killed."),
            DeathReason::UNKNOWN => println!("VM died for an unknown reason."),
            DeathReason::SHUTDOWN => println!("VM shutdown cleanly."),
            DeathReason::ERROR => println!("Error starting VM."),
            DeathReason::REBOOT => println!("VM tried to reboot, possibly due to a kernel panic."),
            DeathReason::CRASH => println!("VM crashed."),
            _ => println!("VM died for an unrecognised reason."),
        }
        Ok(())
    }
}

/// Safely duplicate the standard output file descriptor.
fn duplicate_stdout() -> io::Result<File> {
    let stdout_fd = io::stdout().as_raw_fd();
    // Safe because this just duplicates a file descriptor which we know to be valid, and we check
    // for an error.
    let dup_fd = unsafe { libc::dup(stdout_fd) };
    if dup_fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        // Safe because we have just duplicated the file descriptor so we own it, and `from_raw_fd`
        // takes ownership of it.
        Ok(unsafe { File::from_raw_fd(dup_fd) })
    }
}
