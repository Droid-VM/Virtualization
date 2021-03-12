// Copyright 2021, The Android Open Source Project
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

//! Functions for running instances of `crosvm`.

use crate::aidl::VirtualMachineCallbacks;
use crate::config::VmConfig;
use crate::Cid;
use anyhow::Error;
use log::{error, info};
use shared_child::SharedChild;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self};

const CROSVM_PATH: &str = "/apex/com.android.virt/bin/crosvm";

/// Information about a particular instance of a VM which is running.
#[derive(Debug)]
pub struct VmInstance {
    /// The crosvm child process.
    child: SharedChild,
    /// Whether the VM is still running.
    running: AtomicBool,
    /// The CID assigned to the VM for vsock communication.
    pub cid: Cid,
    /// The filename of the config file that was used to start the VM. This may have changed since
    /// it was read so it shouldn't be trusted; it is only stored for debugging purposes.
    pub config_path: String,
    /// Callbacks to clients of the VM.
    pub callbacks: VirtualMachineCallbacks,
}

impl VmInstance {
    /// Create a new `VmInstance` for the given process.
    fn new(child: SharedChild, cid: Cid, config_path: &str) -> VmInstance {
        VmInstance {
            child,
            cid,
            config_path: config_path.to_owned(),
            running: AtomicBool::new(true),
            callbacks: Default::default(),
        }
    }

    /// Start an instance of `crosvm` to manage a new VM. The `crosvm` instance will be killed when
    /// the `VmInstance` is dropped.
    pub fn start(config: &VmConfig, cid: Cid, config_path: &str) -> Result<Arc<VmInstance>, Error> {
        let child = run_vm(config, cid)?;
        let instance = Arc::new(VmInstance::new(child, cid, config_path));

        let instance_clone = instance.clone();
        thread::spawn(move || {
            instance_clone.monitor();
        });

        Ok(instance)
    }

    /// Wait for the crosvm child process to finish, then mark the VM as no longer running and call
    /// any callbacks.
    fn monitor(&self) {
        match self.child.wait() {
            Err(e) => error!("Error waiting for crosvm instance to die: {}", e),
            Ok(status) => info!("crosvm exited with status {}", status),
        }
        self.running.store(false, Ordering::Release);
        self.callbacks.callback_on_died();
    }

    /// Return whether `crosvm` is still running the VM.
    pub fn running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// Kill the crosvm instance.
    pub fn kill(&self) {
        // TODO: Talk to crosvm to shutdown cleanly.
        if let Err(e) = self.child.kill() {
            error!("Error killing crosvm instance: {}", e);
        }
    }
}

/// Start an instance of `crosvm` to manage a new VM.
fn run_vm(config: &VmConfig, cid: Cid) -> Result<SharedChild, Error> {
    config.validate()?;

    let mut command = Command::new(CROSVM_PATH);
    // TODO(qwandor): Remove --disable-sandbox.
    command.arg("run").arg("--disable-sandbox").arg("--cid").arg(cid.to_string());
    // TODO(jiyong): Don't redirect console to the host syslog
    command.arg("--serial=type=syslog");
    if let Some(bootloader) = &config.bootloader {
        command.arg("--bios").arg(bootloader);
    }
    if let Some(initrd) = &config.initrd {
        command.arg("--initrd").arg(initrd);
    }
    if let Some(params) = &config.params {
        command.arg("--params").arg(params);
    }
    for disk in &config.disks {
        command.arg(if disk.writable { "--rwdisk" } else { "--disk" }).arg(&disk.image);
    }
    if let Some(kernel) = &config.kernel {
        command.arg(kernel);
    }
    info!("Running {:?}", command);
    Ok(SharedChild::spawn(&mut command)?)
}
