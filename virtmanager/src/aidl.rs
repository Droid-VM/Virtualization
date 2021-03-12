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

//! Implementation of the AIDL interface of the Virt Manager.

use crate::config::VmConfig;
use crate::crosvm::VmInstance;
use crate::{Cid, FIRST_GUEST_CID};
use ::binder::FromIBinder;
use android_system_virtmanager::aidl::android::system::virtmanager::IVirtManager::IVirtManager;
use android_system_virtmanager::aidl::android::system::virtmanager::IVirtualMachine::{
    BnVirtualMachine, IVirtualMachine,
};
use android_system_virtmanager::aidl::android::system::virtmanager::IVirtualMachineCallback::IVirtualMachineCallback;
use android_system_virtmanager::aidl::android::system::virtmanager::VirtualMachineDebugInfo::VirtualMachineDebugInfo;
use android_system_virtmanager::binder::{self, Interface, StatusCode, Strong, ThreadState};
use log::{debug, error};
use std::sync::{Arc, Mutex, Weak};

pub const BINDER_SERVICE_IDENTIFIER: &str = "android.system.virtmanager";

// TODO(qwandor): Use PermissionController once it is available to Rust.
/// Only processes running with one of these UIDs are allowed to call debug methods.
const DEBUG_ALLOWED_UIDS: [u32; 2] = [0, 2000];

/// Implementation of `IVirtManager`, the entry point of the AIDL service.
#[derive(Debug, Default)]
pub struct VirtManager {
    state: Mutex<State>,
}

impl Interface for VirtManager {}

impl IVirtManager for VirtManager {
    /// Create and start a new VM with the given configuration, assigning it the next available CID.
    ///
    /// Returns a binder `IVirtualMachine` object referring to it, as a handle for the client.
    fn startVm(&self, config_path: &str) -> binder::Result<Strong<dyn IVirtualMachine>> {
        let state = &mut *self.state.lock().unwrap();

        let cid = state.get_cid()?;
        let instance = start_vm(config_path, cid)?;
        state.add_vm(Arc::downgrade(&instance));
        Ok(VirtualMachine::create(instance))
    }

    /// Get a list of all currently running VMs. This method is only intended for debug purposes,
    /// and as such is only permitted from the shell user.
    fn debugListVms(&self) -> binder::Result<Vec<VirtualMachineDebugInfo>> {
        if !debug_access_allowed() {
            return Err(StatusCode::PERMISSION_DENIED.into());
        }

        let state = &mut *self.state.lock().unwrap();
        let vms = state.vms();
        let cids = vms
            .into_iter()
            .map(|vm| VirtualMachineDebugInfo {
                cid: vm.cid as i32,
                configPath: vm.config_path.clone(),
                running: vm.running(),
            })
            .collect();
        Ok(cids)
    }
}

/// Check whether the caller of the current Binder method is allowed to call debug methods.
fn debug_access_allowed() -> bool {
    let uid = ThreadState::get_calling_uid();
    log::trace!("Debug method call from UID {}.", uid);
    DEBUG_ALLOWED_UIDS.contains(&uid)
}

/// Implementation of the AIDL `IVirtualMachine` interface. Used as a handle to a VM.
#[derive(Debug)]
struct VirtualMachine {
    instance: Arc<VmInstance>,
}

impl VirtualMachine {
    fn create(instance: Arc<VmInstance>) -> Strong<dyn IVirtualMachine> {
        let binder = VirtualMachine { instance };
        BnVirtualMachine::new_binder(binder)
    }
}

impl Interface for VirtualMachine {}

impl IVirtualMachine for VirtualMachine {
    fn getCid(&self) -> binder::Result<i32> {
        Ok(self.instance.cid as i32)
    }

    fn isRunning(&self) -> binder::Result<bool> {
        Ok(self.instance.running())
    }

    fn registerCallback(&self, callback: &dyn IVirtualMachineCallback) -> binder::Result<()> {
        let callback = FromIBinder::try_from(callback.as_binder()).unwrap();
        // TODO: Should this give an error if the VM is already dead?
        self.instance.callbacks.add(callback);
        Ok(())
    }
}

impl Drop for VirtualMachine {
    fn drop(&mut self) {
        debug!("Dropping {:?}", self);
        self.instance.kill();
    }
}

/// A set of Binders to be called back in response to various events on the VM, such as when it
/// dies.
#[derive(Debug, Default)]
pub struct VirtualMachineCallbacks(Mutex<Vec<Strong<dyn IVirtualMachineCallback>>>);

impl VirtualMachineCallbacks {
    /// Call all registered callbacks to say that the VM has died.
    pub fn callback_on_died(&self) {
        let callbacks = &*self.0.lock().unwrap();
        for callback in callbacks {
            if let Err(e) = callback.onDied() {
                error!("Error calling callback: {}", e);
            }
        }
    }

    /// Add a new callback to the set.
    fn add(&self, callback: Strong<dyn IVirtualMachineCallback>) {
        self.0.lock().unwrap().push(callback);
    }
}

/// The mutable state of the Virt Manager. There should only be one instance of this struct.
#[derive(Debug)]
struct State {
    /// The next available unused CID.
    next_cid: Cid,

    /// The VMs which have been started. When VMs are started a weak reference is added to this list
    /// while a strong reference is returned to the caller over Binder. Once all copies of the
    /// Binder client are dropped the weak reference here will become invalid, and will be removed
    /// from the list opportunistically the next time `add_vm` is called.
    vms: Vec<Weak<VmInstance>>,
}

impl State {
    /// Get a list of VMs which still have Binder references to them.
    fn vms(&self) -> Vec<Arc<VmInstance>> {
        // Attempt to upgrade the weak pointers to strong pointers.
        self.vms.iter().filter_map(Weak::upgrade).collect()
    }

    /// Add a new VM to the list.
    fn add_vm(&mut self, vm: Weak<VmInstance>) {
        // Garbage collect any entries from the stored list which no longer exist.
        self.vms.retain(|vm| vm.strong_count() > 0);

        // Actually add the new VM.
        self.vms.push(vm);
    }

    /// Get the next available CID, or an error if we have run out.
    fn get_cid(&mut self) -> binder::Result<Cid> {
        // TODO(qwandor): keep track of which CIDs are currently in use so that we can reuse them.
        let cid = self.next_cid;
        self.next_cid = self.next_cid.checked_add(1).ok_or(StatusCode::UNKNOWN_ERROR)?;
        Ok(cid)
    }
}

impl Default for State {
    fn default() -> Self {
        State { next_cid: FIRST_GUEST_CID, vms: vec![] }
    }
}

/// Start a new VM instance from the given VM config filename. This assumes the VM is not already
/// running.
fn start_vm(config_path: &str, cid: Cid) -> binder::Result<Arc<VmInstance>> {
    let config = VmConfig::load(config_path).map_err(|e| {
        error!("Failed to load VM config {}: {:?}", config_path, e);
        StatusCode::BAD_VALUE
    })?;
    Ok(VmInstance::start(&config, cid, config_path).map_err(|e| {
        error!("Failed to start VM {}: {:?}", config_path, e);
        StatusCode::UNKNOWN_ERROR
    })?)
}
