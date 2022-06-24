/*
 * Copyright (C) 2022 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Mock up of an RKP service VM for VMs.

mod rkpvm;

use android_system_virtualmachineservice::{
    aidl::android::system::virtualmachineservice::IVirtualMachineService::{
        IVirtualMachineService, VM_BINDER_SERVICE_PORT,
    },
    binder::Strong,
};
use anyhow::{anyhow, bail, Context, Result};
use binder::{
    unstable_api::{new_spibinder, AIBinder},
    FromIBinder,
};
use binder_common::rpc_server::run_rpc_server;
use log::{debug, error};
use std::os::raw::c_int;
use std::panic;

/// The CID representing the host VM
const VMADDR_CID_HOST: u32 = 2;

/// Entry point called from microdroid_launcher.
#[no_mangle]
pub extern "C" fn android_native_main(_argc: c_int, _argv: *const *const char) -> c_int {
    if let Err(e) = try_main() {
        error!("failed with {:?}", e);
        return 1;
    }
    0
}

fn try_main() -> Result<()> {
    android_logger::init_once(
        android_logger::Config::default().with_tag("rkpvm").with_min_level(log::Level::Debug),
    );
    // Redirect panic messages to logcat.
    panic::set_hook(Box::new(|panic_info| {
        error!("{}", panic_info);
    }));

    let service = rkpvm::new_binder()?.as_binder();
    let vm_service = get_vm_service()?;

    debug!("rkpvm is starting as a rpc service.");

    let retval = run_rpc_server(service, 6433, || {
        if let Err(e) = vm_service.notifyPayloadReady() {
            error!("Unable to notify ready: {}", e);
        }
    });
    if retval {
        debug!("RPC server has shut down gracefully");
        Ok(())
    } else {
        bail!("Premature termination of RPC server");
    }
}

fn get_vm_service() -> Result<Strong<dyn IVirtualMachineService>> {
    // SAFETY: AIBinder returned by RpcClient has correct reference count, and the ownership
    // can be safely taken by new_spibinder.
    let ibinder = unsafe {
        new_spibinder(binder_rpc_unstable_bindgen::RpcClient(
            VMADDR_CID_HOST,
            VM_BINDER_SERVICE_PORT as u32,
        ) as *mut AIBinder)
    }
    .ok_or_else(|| anyhow!("Failed to connect to IVirtualMachineService"))?;

    FromIBinder::try_from(ibinder).context("Connecting to IVirtualMachineService")
}
