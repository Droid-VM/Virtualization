//! Functions for running instances of `crosvm`.

use crate::config::VmConfig;
use crate::Cid;
use anyhow::{bail, Error};
use log::{debug, error, info};
use std::process::{Child, Command};

const CROSVM_PATH: &str = "/apex/com.android.virt/bin/crosvm";

/// Information about a particular instance of a VM which is running.
#[derive(Debug)]
pub struct VmInstance {
    /// The crosvm child process.
    child: Child,
    /// The CID assigned to the VM for vsock communication.
    pub cid: Cid,
}

impl VmInstance {
    /// Create a new `VmInstance` for the given process.
    fn new(child: Child, cid: Cid) -> VmInstance {
        VmInstance { child, cid }
    }

    /// Start an instance of `crosvm` to manage a new VM. The `crosvm` instance will be killed when
    /// the `VmInstance` is dropped.
    pub fn start(config: &VmConfig, cid: Cid) -> Result<VmInstance, Error> {
        let child = run_vm(config, cid)?;
        Ok(VmInstance::new(child, cid))
    }
}

impl Drop for VmInstance {
    fn drop(&mut self) {
        debug!("Dropping {:?}", self);
        // TODO: Talk to crosvm to shutdown cleanly.
        if let Err(e) = self.child.kill() {
            error!("Error killing crosvm instance: {}", e);
        }
        // We need to wait on the process after killing it to avoid zombies.
        match self.child.wait() {
            Err(e) => error!("Error waiting for crosvm instance to die: {}", e),
            Ok(status) => info!("Crosvm exited with status {}", status),
        }
    }
}

/// Start an instance of `crosvm` to manage a new VM.
fn run_vm(config: &VmConfig, cid: Cid) -> Result<Child, Error> {
    if config.bootloader.is_none() && config.kernel.is_none() {
        bail!("VM must have either a bootloader or a kernel image.");
    }
    if config.bootloader.is_some() && (config.kernel.is_some() || config.initrd.is_some()) {
        bail!("Can't have both bootloader and kernel/initrd image.");
    }

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
    // TODO: Monitor child process, and remove from VM map if it dies.
    Ok(command.spawn()?)
}
