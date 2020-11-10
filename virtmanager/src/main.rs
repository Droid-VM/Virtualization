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
const FIRST_GUEST_CID: Cid = 4;

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
    /// The Binder VirtualMachine object for the VM.
    /// This should never actually be None once `new` returns, it is just needed for initialisation.
    vm_binder: Mutex<Option<WpIBinder>>,
    /// A reference to the global state object, so that the VM can remove itself when it dies.
    state: Arc<Mutex<State>>,
    /// The config path with which this VM was started, so that it can remove itself from the
    /// hashmap.
    config_path: String,
}

impl VmInstance {
    /// Create a new `VmInstance` with a single reference for the given process.
    fn new(
        child: Child,
        cid: Cid,
        state: Arc<Mutex<State>>,
        config_path: &str,
    ) -> (Arc<VmInstance>, Box<dyn IVirtualMachine>) {
        let instance = Arc::new(VmInstance {
            child,
            cid,
            vm_binder: Mutex::new(None),
            state,
            config_path: config_path.to_owned(),
        });
        let binder = VirtualMachine { instance: instance.clone() };
        let binder_strong = Box::new(BnVirtualMachine::new_binder(binder));
        let binder_weak = binder_strong.as_binder().downgrade();
        *instance.vm_binder.lock().unwrap() = Some(binder_weak);
        (instance, binder_strong)
    }

    // Remove VmInstance from VM hashmap. This will result in it being dropped, and thus being
    // shut down.
    fn remove(&self) {
        // TODO: Wait for some timeout before doing this.
        self.state.lock().unwrap().vms.remove(&self.config_path);
    }

    fn binder(self: &Arc<Self>) -> Box<dyn IVirtualMachine> {
        let vm_binder = &mut *self.vm_binder.lock().unwrap();
        // vm_binder should never be None once `new` returns, so the `unwrap` is safe.
        let vm_binder = vm_binder.as_mut().unwrap();
        if let Some(vm_strong) = vm_binder.promote() {
            vm_strong.into_interface::<dyn IVirtualMachine>().unwrap()
        } else {
            let new_vm_binder = VirtualMachine { instance: self.clone() };
            let new_vm_binder_strong = Box::new(BnVirtualMachine::new_binder(new_vm_binder));
            *vm_binder = new_vm_binder_strong.as_binder().downgrade();
            new_vm_binder_strong
        }
    }
}

impl Drop for VmInstance {
    fn drop(&mut self) {
        info!("Dropping {:?}", self);
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

#[derive(Debug)]
struct VirtualMachine {
    instance: Arc<VmInstance>,
}

impl Interface for VirtualMachine {}

impl IVirtualMachine for VirtualMachine {
    fn get_cid(&self) -> binder::Result<i32> {
        Ok(self.instance.cid as i32)
    }
}

impl Drop for VirtualMachine {
    fn drop(&mut self) {
        info!("Dropping {:?}", self);
        self.instance.remove();
    }
}

/// The mutable state of the Virt Manager. There should only be one instance of this struct.
#[derive(Debug)]
struct State {
    vms: HashMap<String, Arc<VmInstance>>,
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

    /// Create and start a new VM, assigning it the next available CID.
    /// Assumes that the VM is not already running, and doesn't insert it into the VM map.
    /// Returns the VmInstance to be inserted into the VM map, and a binder object referring to it.
    fn create_vm(
        &self,
        config_path: &str,
        next_cid: &mut Cid,
    ) -> binder::Result<(Arc<VmInstance>, Box<dyn IVirtualMachine>)> {
        let cid = *next_cid;
        let child = start_vm(config_path, cid)?;
        *next_cid += 1;
        let state = self.state.clone();
        Ok(VmInstance::new(child, cid, state, config_path))
    }
}

impl Interface for VirtManager {}

impl IVirtManager for VirtManager {
    /// Checks whether the VM with the given config_path is already running. If so, returns a
    /// reference to it. If not, creates and starts the VM and inserts it into the VM map.
    fn start_vm(&self, config_path: &str) -> binder::Result<Box<dyn IVirtualMachine>> {
        let state = &mut *self.state.lock().unwrap();
        let vm = match state.vms.entry(config_path.to_owned()) {
            Entry::Occupied(occupied) => {
                let vm = occupied.into_mut();
                vm.binder()
            }
            Entry::Vacant(vacant) => {
                let (vm, binder) = self.create_vm(config_path, &mut state.next_cid)?;
                vacant.insert(vm);
                binder
            }
        };
        Ok(vm)
    }
}

/// Start a new VM instance from the given VM config filename. This assumes the VM is not already
/// running.
fn start_vm(config_path: &str, cid: Cid) -> binder::Result<Child> {
    let config = load_vm_config(config_path).map_err(|e| {
        error!("Failed to load VM config {}: {:?}", config_path, e);
        StatusCode::BAD_VALUE
    })?;
    let child = run_vm(&config, cid).map_err(|e| {
        error!("Failed to start VM {}: {:?}", config_path, e);
        StatusCode::UNKNOWN_ERROR
    })?;
    Ok(child)
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
    // TODO: Monitor child process, and remove from VM map if it dies.
    let child = command.spawn()?;
    Ok(child)
}
