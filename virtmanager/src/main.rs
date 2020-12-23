//! Android Virt Manager

use android_system_virtmanager::aidl::android::system::virtmanager::IVirtManager::{
    BnVirtManager, IVirtManager,
};
use android_system_virtmanager::aidl::android::system::virtmanager::IVirtualMachine::{
    BnVirtualMachine, IVirtualMachine,
};
use android_system_virtmanager::binder::{self, add_service, Interface, StatusCode, WpIBinder};
use anyhow::{Context, Error};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};

/// The first CID to assign to a guest VM managed by the Virt Manager. CIDs lower than this are
/// reserved for the host or other usage.
const FIRST_GUEST_CID: Cid = 1;

const BINDER_SERVICE_IDENTIFIER: &str = "android.system.virtmanager";

fn main() {
    env_logger::init();
    let state = Arc::new(Mutex::new(State::new()));
    let virt_manager = VirtManager::new(state);
    let virt_manager = BnVirtManager::new_binder(virt_manager);
    add_service(BINDER_SERVICE_IDENTIFIER, virt_manager.as_binder()).unwrap();
    info!("Registered Binder service, joining threadpool.");
    binder::ProcessState::join_thread_pool();
}

/// The unique ID of a VM used (together with a port number) for vsock communication.
type Cid = u32;

/// Configuration for a particular VM to be started.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct VmConfig {
    kernel: String,
    initrd: Option<String>,
    params: Option<String>,
}

/// Information about a particular instance of a VM which is running.
#[derive(Debug)]
struct VmInstance {
    /// The crosvm child process.
    child: Child,
    /// The CID assigned to the VM for vsock communication.
    cid: Cid,
}

impl VmInstance {
    /// Create a new `VmInstance` with a single reference for the given process.
    fn new(child: Child, cid: Cid) -> VmInstance {
        VmInstance { child, cid }
    }
}

impl Drop for VmInstance {
    fn drop(&mut self) {
        info!("Dropping VmInstance with CID {}", self.cid);
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

impl Interface for VmInstance {}

impl IVirtualMachine for VmInstance {
    fn get_cid(&self) -> binder::Result<i32> {
        Ok(self.cid as i32)
    }
}

/// The mutable state of the Virt Manager. There should only be one instance of this struct.
#[derive(Debug)]
struct State {
    vms: HashMap<String, WpIBinder>,
    next_cid: Cid,
}

impl State {
    fn new() -> Self {
        State { vms: HashMap::new(), next_cid: FIRST_GUEST_CID }
    }
}

#[derive(Debug)]
struct VirtManager {
    state: Arc<Mutex<State>>,
}

impl VirtManager {
    fn new(state: Arc<Mutex<State>>) -> Self {
        VirtManager { state }
    }
}

impl Interface for VirtManager {}

impl IVirtManager for VirtManager {
    fn start_vm(&self, config_path: &str) -> binder::Result<Box<dyn IVirtualMachine>> {
        let state = &mut *self.state.lock().unwrap();
        let vm = match state.vms.entry(config_path.to_owned()) {
            Entry::Occupied(occupied) => {
                let vm_weak = occupied.into_mut();
                if let Some(vm_strong) = vm_weak.promote() {
                    vm_strong.into_interface::<dyn IVirtualMachine>().unwrap()
                } else {
                    // TODO: Ensure that VM has actually been shut down before re-lauching it.
                    let vm = create_vm(config_path, &mut state.next_cid)?;
                    *vm_weak = vm.as_binder().downgrade();
                    vm
                }
            }
            Entry::Vacant(vacant) => {
                let vm = create_vm(config_path, &mut state.next_cid)?;
                vacant.insert(vm.as_binder().downgrade());
                vm
            }
        };
        Ok(vm)
    }
}

fn create_vm(config_path: &str, next_cid: &mut Cid) -> binder::Result<Box<dyn IVirtualMachine>> {
    let instance = start_vm(config_path, *next_cid)?;
    *next_cid += 1;
    Ok(Box::new(BnVirtualMachine::new_binder(instance)))
}

/// Start a new VM instance from the given VM config filename. This assumes the VM is not already
/// running.
fn start_vm(config_path: &str, cid: Cid) -> binder::Result<VmInstance> {
    let config = load_vm_config(config_path).map_err(|e| {
        error!("Failed to load VM config {}: {:?}", config_path, e);
        StatusCode::BAD_VALUE
    })?;
    let child = run_vm(&config, cid).map_err(|e| {
        error!("Failed to start VM {}: {:?}", config_path, e);
        StatusCode::UNKNOWN_ERROR
    })?;
    Ok(VmInstance::new(child, cid))
}

/// Load the configuration for the VM with the given ID from a JSON file.
fn load_vm_config(path: &str) -> Result<VmConfig, Error> {
    let file = File::open(path).with_context(|| format!("Failed to open {}", path))?;
    let buffered = BufReader::new(file);
    Ok(serde_json::from_reader(buffered)?)
}

/// Start an instance of `crosvm` to manage a new VM.
fn run_vm(config: &VmConfig, cid: u32) -> Result<Child, io::Error> {
    let mut command = Command::new("crosvm");
    command.arg("run").arg("--disable-sandbox").arg("--cid").arg(cid.to_string());
    if let Some(initrd) = &config.initrd {
        command.arg("--initrd").arg(initrd);
    }
    if let Some(params) = &config.params {
        command.arg("--params").arg(params);
    }
    command.arg(&config.kernel);
    info!("Running {:?}", command);
    let child = command.spawn()?;
    Ok(child)
}
