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

//! This module handles the interaction with vsock server.

use android_system_virtualmachineservice::aidl::android::system::virtualmachineservice::IVirtualMachineService::{
    VM_BINDER_SERVICE_PORT, IVirtualMachineService,
};
use anyhow::{Context, Result};
use binder::Strong;
use rpcbinder::get_vsock_rpc_interface;
use std::mem::ManuallyDrop;
use std::rc::Rc;

/// The CID representing the host VM
const VMADDR_CID_HOST: u32 = 2;

/// Vsock server for payload.
pub struct VsockServer {
    #[allow(dead_code)]
    virtual_machine_service: Strong<dyn IVirtualMachineService>,
}

/// Sets up the `VsockServer`.
#[no_mangle]
pub extern "C" fn setup_vsock_server() -> *const VsockServer {
    let vsock_server =
        VsockServer { virtual_machine_service: get_virtual_machine_service().unwrap() };
    &vsock_server
}

/// Notifies the `VsockServer` that the payload is ready.
/// # Safety
/// - This should be called from the thread of creation
/// - `vsock_server` must be a valid pointer
#[no_mangle]
pub unsafe extern "C" fn notify_payload_ready(vsock_server: *const VsockServer) -> bool {
    let vsock_server = ManuallyDrop::new(Rc::from_raw(vsock_server));
    vsock_server.virtual_machine_service.notifyPayloadReady().is_ok()
}

fn get_virtual_machine_service() -> Result<Strong<dyn IVirtualMachineService>> {
    get_vsock_rpc_interface(VMADDR_CID_HOST, VM_BINDER_SERVICE_PORT as u32)
        .context("Cannot connect to VirtualMachineService")
}
