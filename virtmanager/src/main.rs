//! Android Virt Manager

use android_system_virtmanager::aidl::android::system::virtmanager::IVirtManager::IVirtManager;
use android_system_virtmanager::binder::{self, Interface};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The first CID to assign to a guest VM managed by the Virt Manager. CIDs lower than this are
/// reserved for the host or other usage.
const FIRST_GUEST_CID: Cid = 1;

fn main() {
    let state = Arc::new(Mutex::new(State::new()));
    let _virt_manager = VirtManager::new(state);
}

/// The unique ID of a VM used (together with a port number) for vsock communication.
type Cid = u32;

/// The mutable state of the Virt Manager. There should only be one instance of this struct.
struct State {
    vms: HashMap<String, Cid>,
    next_cid: Cid,
}

impl State {
    fn new() -> Self {
        State { vms: HashMap::new(), next_cid: FIRST_GUEST_CID }
    }
}

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
        let mut state = self.state.lock().unwrap();
        if let Some(&existing_cid) = state.vms.get(vm_id) {
            Ok(existing_cid as i32)
        } else {
            let cid = state.next_cid;
            state.next_cid += 1;
            state.vms.insert(vm_id.to_owned(), cid);
            Ok(cid as i32)
        }
    }
}
