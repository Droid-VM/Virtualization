//! Android Virt Manager

use android_system_virtmanager::aidl::android::system::virtmanager::IVirtManager::{
    BnVirtManager, IVirtManager,
};
use android_system_virtmanager::binder::{self, add_service, Interface, StatusCode};
use anyhow::{Context, Error};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};

/// The first CID to assign to a guest VM managed by the Virt Manager. CIDs lower than this are
/// reserved for the host or other usage.
const FIRST_GUEST_CID: Cid = 1;

const BINDER_SERVICE_IDENTIFIER: &str = "android.system.virtmanager";
const VM_CONFIG_DIRECTORY: &str = "/data/vms";

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
#[derive(Clone, Debug, Eq, PartialEq)]
struct VmInstance {
    // The CID assigned to the VM for vsock communication.
    cid: Cid,
    // The reference counts to the VM, for each process which called start_vm for it.
    references: HashMap<u64, u64>,
}

impl VmInstance {
    /// Create a new `VmInstance` with a single reference for the given process.
    fn new(cid: Cid, pid: u64) -> VmInstance {
        let mut references = HashMap::new();
        references.insert(pid, 1);
        VmInstance { cid, references }
    }

    fn add_reference(&mut self, pid: u64) {
        *self.references.entry(pid).or_insert(0) += 1;
    }

    fn no_remaining_references(&self) -> bool {
        self.references.values().sum::<u64>() == 0
    }

    /// Decrement the reference count for the given process to this VM instance.
    ///
    /// Returns true if there are no references remaining.
    fn remove_reference(&mut self, pid: u64) -> Result<bool, StatusCode> {
        if let Some(ref_count) = self.references.get_mut(&pid) {
            *ref_count -= 1;
            Ok(self.no_remaining_references())
        } else {
            error!("PID {} tried to remove reference but there were none.", pid);
            Err(StatusCode::UNKNOWN_ERROR)
        }
    }

    /*fn remove_all_references(&mut self, pid: u64) -> bool {
        self.references.remove(&pid);
        self.no_remaining_references()
    }*/
}

/// The mutable state of the Virt Manager. There should only be one instance of this struct.
#[derive(Debug)]
struct State {
    vms: HashMap<String, VmInstance>,
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
    fn start_vm(&self, vm_id: &str) -> binder::Result<i32> {
        let pid = 0; // TODO
        let mut state = self.state.lock().unwrap();
        if let Some(existing_instance) = state.vms.get_mut(vm_id) {
            existing_instance.add_reference(pid);
            Ok(existing_instance.cid as i32)
        } else {
            let cid = state.next_cid;
            let config = load_vm_config(vm_id).map_err(|e| {
                error!("Failed to load VM config {}: {:?}", vm_id, e);
                StatusCode::UNKNOWN_ERROR
            })?;
            run_vm(&config, cid).map_err(|e| {
                error!("Failed to start VM {}: {:?}", vm_id, e);
                StatusCode::UNKNOWN_ERROR
            })?;
            state.next_cid += 1;
            state.vms.insert(vm_id.to_owned(), VmInstance::new(cid, pid));
            Ok(cid as i32)
        }
    }

    fn drop_vm_reference(&self, vm_id: &str) -> binder::Result<()> {
        let pid = 0; // TODO
        let mut state = self.state.lock().unwrap();
        if let Some(existing_instance) = state.vms.get_mut(vm_id) {
            existing_instance.remove_reference(pid)?;
            Ok(())
        } else {
            error!("Tried to drop VM reference for VM {} which doesn't exist.", vm_id);
            Err(StatusCode::UNKNOWN_ERROR.into())
        }
    }
}

/// Load the configuration for the VM with the given ID from a JSON file.
fn load_vm_config(vm_id: &str) -> Result<VmConfig, Error> {
    let filename = format!("{}/{}.json", VM_CONFIG_DIRECTORY, vm_id);
    let file = File::open(&filename).with_context(|| format!("Failed to open {}", filename))?;
    let buffered = BufReader::new(file);
    Ok(serde_json::from_reader(buffered)?)
}

/// Start an instance of `crosvm` to manage a new VM.
fn run_vm(config: &VmConfig, cid: u32) -> Result<Child, io::Error> {
    let mut command = Command::new("crosvm");
    command.arg("run").arg(&config.kernel).arg("--cid").arg(cid.to_string());
    if let Some(initrd) = &config.initrd {
        command.arg("--initrd").arg(initrd);
    }
    if let Some(params) = &config.params {
        command.arg("--params").arg(params);
    }
    info!("Running {:?}", command);
    let child = command.spawn()?;
    Ok(child)
}
