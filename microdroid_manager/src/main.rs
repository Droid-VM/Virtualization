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

//! Microdroid Manager

mod ioutil;
mod metadata;

use anyhow::{anyhow, bail, Context, Result};
use apkverify::verify;
use binder::unstable_api::{new_spibinder, AIBinder};
use binder::{FromIBinder, Strong};
use libc::splice;
use log::{error, info, warn};
use microdroid_payload_config::{Task, TaskType, VmPayloadConfig};
use nix::ioctl_read_bad;
use rustutils::system_properties::PropertyWatcher;
use std::convert::Into;
use std::fs::{self, OpenOptions};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::process::{Command, Stdio};
use std::ptr::null_mut;
use std::str;
use std::time::Duration;
use vsock::VsockListener;

use android_system_virtualmachineservice::aidl::android::system::virtualmachineservice::IVirtualMachineService::IVirtualMachineService;

const WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const DM_MOUNTED_APK_PATH: &str = "/dev/block/mapper/microdroid-apk";

/// The CID representing the host VM
const VMADDR_CID_HOST: u32 = 2;

/// Port number that virtualizationservice listens on connections from the guest VMs for the
/// VirtualMachineService binder service
/// Sync with virtualizationservice/src/aidl.rs
const PORT_VM_BINDER_SERVICE: u32 = 8000;

/// Port numbers for the stdout/stderr server from the guest VM.
const PORT_VM_STDOUT: u32 = 3001;
const PORT_VM_STDERR: u32 = 3002;

fn get_vms_rpc_binder() -> Result<Strong<dyn IVirtualMachineService>> {
    // SAFETY: AIBinder returned by RpcClient has correct reference count, and the ownership can be
    // safely taken by new_spibinder.
    let ibinder = unsafe {
        new_spibinder(binder_rpc_unstable_bindgen::RpcClient(
            VMADDR_CID_HOST,
            PORT_VM_BINDER_SERVICE,
        ) as *mut AIBinder)
    };
    if let Some(ibinder) = ibinder {
        <dyn IVirtualMachineService>::try_from(ibinder).context("Cannot connect to RPC service")
    } else {
        bail!("Invalid raw AIBinder")
    }
}

ioctl_read_bad!(
    /// IOCTL_VM_SOCKETS_GET_LOCAL_CID
    _vm_sockets_get_local_cid,
    0x7b9,
    u32
);

fn get_local_cid() -> Result<u32> {
    let f = OpenOptions::new()
        .read(true)
        .write(false)
        .open("/dev/vsock")
        .context("failed to open /dev/vsock")?;
    let mut ret = 0;
    // SAFETY: the kernel only modifies the given u32 integer.
    unsafe { _vm_sockets_get_local_cid(f.as_raw_fd(), &mut ret) }?;
    Ok(ret)
}

fn main() -> Result<()> {
    kernlog::init()?;
    info!("started.");

    let metadata = metadata::load()?;

    if let Err(err) = verify_payloads() {
        error!("failed to verify payload: {:#?}", err);
        return Err(err);
    }

    // TODO(b/191845268): microdroid_manager should use this binder to communicate with the host
    if let Err(err) = get_vms_rpc_binder() {
        error!("cannot connect VirtualMachineService: {}", err);
    }

    let service = get_vms_rpc_binder().expect("cannot connect VirtualMachineService");

    if !metadata.payload_config_path.is_empty() {
        let config = load_config(Path::new(&metadata.payload_config_path))?;

        let fake_secret = "This is a placeholder for a value that is derived from the images that are loaded in the VM.";
        if let Err(err) = rustutils::system_properties::write("ro.vmsecret.keymint", fake_secret) {
            warn!("failed to set ro.vmsecret.keymint: {}", err);
        }

        // TODO(jooyung): wait until sys.boot_completed?
        if let Some(main_task) = &config.task {
            exec_task(main_task, &service).map_err(|e| {
                error!("failed to execute task: {}", e);
                e
            })?;
        }
    }

    Ok(())
}

// TODO(jooyung): v2/v3 full verification can be slow. Consider multithreading.
fn verify_payloads() -> Result<()> {
    // We don't verify APEXes since apexd does.

    // should wait APK to be dm-verity mounted by apkdmverity
    ioutil::wait_for_file(DM_MOUNTED_APK_PATH, WAIT_TIMEOUT)?;
    verify(DM_MOUNTED_APK_PATH).context(format!("failed to verify {}", DM_MOUNTED_APK_PATH))?;

    info!("payload verification succeeded.");
    // TODO(jooyung): collect public keys and store them in instance.img
    Ok(())
}

fn load_config(path: &Path) -> Result<VmPayloadConfig> {
    info!("loading config from {:?}...", path);
    let file = ioutil::wait_for_file(path, WAIT_TIMEOUT)?;
    Ok(serde_json::from_reader(file)?)
}

fn forward_pipe_as_vsock(pipe: impl AsRawFd, port: u32) -> Result<()> {
    let listener = VsockListener::bind_with_cid_port(u32::MAX, port)?;
    info!("vsock server started listening at port {}", port);
    let (vsock, addr) = listener.accept()?;
    info!("vsock server accepted a connection at port {} from remote addr {}", port, addr);
    let pipe_fd = pipe.as_raw_fd();
    let vsock_fd = vsock.as_raw_fd();
    loop {
        // SAFETY: we pass null to kernel, so kernel doesn't touch anything
        match unsafe { splice(pipe_fd, null_mut(), vsock_fd, null_mut(), 4096, 0) } {
            0 => break,
            -1 => return Err(std::io::Error::last_os_error().into()),
            _ => (),
        };
    }
    Ok(())
}

/// Executes the given task. Stdout of the task is piped into the vsock stream to the
/// virtualizationservice in the host side.
fn exec_task(task: &Task, service: &Strong<dyn IVirtualMachineService>) -> Result<()> {
    info!("executing main task {:?}...", task);
    let mut child = build_command(task)?.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let stdout_thread = std::thread::spawn(move || forward_pipe_as_vsock(stdout, PORT_VM_STDOUT));
    let stderr_thread = std::thread::spawn(move || forward_pipe_as_vsock(stderr, PORT_VM_STDERR));

    // Wait until stdout/stderr servers listens.
    // TODO(inseob): use synchronization method like condition variables
    std::thread::sleep(Duration::from_secs(1));
    info!("notifying payload started");
    service.notifyPayloadStarted(get_local_cid()? as i32)?;

    // Wait, and then join. The host may read outputs after the payload is finished.
    let result = child.wait();

    if let Err(e) = stdout_thread.join().expect("Couldn't join stdout thread") {
        error!("stdout thread exited with error: {}", e);
    }
    if let Err(e) = stderr_thread.join().expect("Couldn't join stderr thread") {
        error!("stderr thread exited with error: {}", e);
    }

    match result?.code() {
        Some(0) => {
            info!("task successfully finished");
            Ok(())
        }
        Some(code) => bail!("task exited with exit code: {}", code),
        None => bail!("task terminated by signal"),
    }
}

fn build_command(task: &Task) -> Result<Command> {
    Ok(match task.type_ {
        TaskType::Executable => {
            let mut command = Command::new(&task.command);
            command.args(&task.args);
            command
        }
        TaskType::MicrodroidLauncher => {
            let mut command = Command::new("/system/bin/microdroid_launcher");
            command.arg(find_library_path(&task.command)?).args(&task.args);
            command
        }
    })
}

fn find_library_path(name: &str) -> Result<String> {
    let mut watcher = PropertyWatcher::new("ro.product.cpu.abilist")?;
    let value = watcher.read(|_name, value| Ok(value.trim().to_string()))?;
    let abi = value.split(',').next().ok_or_else(|| anyhow!("no abilist"))?;
    let path = format!("/mnt/apk/lib/{}/{}", abi, name);

    let metadata = fs::metadata(&path)?;
    if !metadata.is_file() {
        bail!("{} is not a file", &path);
    }

    Ok(path)
}
