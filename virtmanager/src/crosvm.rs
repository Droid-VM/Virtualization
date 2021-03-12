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

use crate::config::VmConfig;
use crate::Cid;
use anyhow::Error;
use log::{debug, error, info};
use shared_child::SharedChild;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

const CROSVM_PATH: &str = "/apex/com.android.virt/bin/crosvm";

/// Information about a particular instance of a VM which is running.
#[derive(Debug)]
pub struct VmInstance {
    /// The crosvm child process.
    child: Arc<SharedChild>,
    /// Thread which waits on the child process.
    thread: JoinHandle<()>,
    /// Whether the VM is still running.
    running: Arc<AtomicBool>,
    /// The CID assigned to the VM for vsock communication.
    pub cid: Cid,
    /// The filename of the config file that was used to start the VM. This may have changed since
    /// it was read so it shouldn't be trusted; it is only stored for debugging purposes.
    pub config_path: String,
}

impl VmInstance {
    /// Create a new `VmInstance` for the given process.
    fn new(
        child: Arc<SharedChild>,
        thread: JoinHandle<()>,
        cid: Cid,
        config_path: &str,
        running: Arc<AtomicBool>,
    ) -> VmInstance {
        VmInstance { child, thread, cid, config_path: config_path.to_owned(), running }
    }

    /// Start an instance of `crosvm` to manage a new VM. The `crosvm` instance will be killed when
    /// the `VmInstance` is dropped.
    pub fn start(config: &VmConfig, cid: Cid, config_path: &str) -> Result<VmInstance, Error> {
        let child = Arc::new(run_vm(config, cid)?);
        let running = Arc::new(AtomicBool::new(true));
        let child_clone = child.clone();
        let running_clone = running.clone();
        let thread = thread::spawn(move || {
            match child_clone.wait() {
                Err(e) => error!("Error waiting for crosvm instance to die: {}", e),
                Ok(status) => info!("Crosvm exited with status {}", status),
            }
            running_clone.store(false, Ordering::Release);
        });
        Ok(VmInstance::new(child, thread, cid, config_path, running))
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
}

impl Drop for VmInstance {
    fn drop(&mut self) {
        debug!("Dropping {:?}", self);
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
