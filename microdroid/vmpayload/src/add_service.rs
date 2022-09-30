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

//! This module is responsible of adding a service from payload to virtual machine
//! service.

use android_system_virtualmachineservice::aidl::android::system::virtualmachineservice::IVirtualMachineService::{
    VM_BINDER_SERVICE_PORT, IVirtualMachineService,
};
use anyhow::{bail, Context, Result};
use binder::{Strong, unstable_api::{AIBinder, new_spibinder}};
use log::{error, info};
use rpcbinder::{get_vsock_rpc_interface, run_rpc_server};

/// The CID representing the host VM
const VMADDR_CID_HOST: u32 = 2;

/// Adds a service to virtual machine service on the given port and then notifies the host.
#[no_mangle]
pub extern "C" fn add_service_to_vms(service: *mut AIBinder, port: i32) {
    try_add_service_to_vms(service, port).unwrap()
}

fn try_add_service_to_vms(service: *mut AIBinder, port: i32) -> Result<()> {
    let service = unsafe { new_spibinder(service).context("Cannot get binder")? };
    let vm_service = get_vm_service()?;
    let ret = run_rpc_server(service, port as u32, || {
        if let Err(e) = vm_service.notifyPayloadReady() {
            error!("Unable to notify ready: {}", e);
        }
    });
    if ret {
        info!("RPC server has shut down gracefully");
        Ok(())
    } else {
        bail!("Premature termination of RPC server");
    }
}

fn get_vm_service() -> Result<Strong<dyn IVirtualMachineService>> {
    get_vsock_rpc_interface(VMADDR_CID_HOST, VM_BINDER_SERVICE_PORT as u32)
        .context("Cannot connect to VirtualMachineService")
}
