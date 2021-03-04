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

//! Android VM control tool.

use android_system_virtmanager::aidl::android::system::virtmanager::IVirtManager::IVirtManager;
use android_system_virtmanager::binder::{
    get_interface, DeathRecipient, IBinder, ProcessState, Strong,
};
use anyhow::{anyhow, bail, Error};
use std::env;
use std::process::exit;
use std::sync::{Arc, Condvar, Mutex};

const VIRT_MANAGER_BINDER_SERVICE_IDENTIFIER: &str = "android.system.virtmanager";

fn main() -> Result<(), Error> {
    env_logger::init();

    let args: Vec<_> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage:");
        eprintln!("  {} run <vm_config.json>", args[0]);
        exit(1);
    }

    // We need to start the thread pool for Binder to work properly, especially link_to_death.
    ProcessState::start_thread_pool();

    match args[1].as_ref() {
        "run" if args.len() == 3 => command_run(&args[2]),
        command => bail!("Invalid command '{}' or wrong number of arguments", command),
    }
}

#[allow(clippy::mutex_atomic)]
fn command_run(config_filename: &str) -> Result<(), Error> {
    // TODO: Stop mapping errors once b/181225442 is fixed.
    let virt_manager: Strong<dyn IVirtManager> =
        get_interface(VIRT_MANAGER_BINDER_SERVICE_IDENTIFIER)
            .map_err(|e| anyhow!("Failed to find Virt Manager service: {}", e))?;
    let vm =
        virt_manager.startVm(config_filename).map_err(|e| anyhow!("Failed to start VM: {}", e))?;
    let cid = vm.getCid().map_err(|e| anyhow!("Failed to get CID: {}", e))?;
    println!("Started VM from {} with CID {}.", config_filename, cid);

    // Wait until VM dies. If we just returned immediately then the IVirtualMachine Binder object
    // would be dropped and the VM would be killed.
    // TODO: The DeathRecipient seems never to be called.
    let vm_died = Arc::new((Mutex::new(false), Condvar::new()));
    let mut death_recipient = {
        let vm_died = vm_died.clone();
        DeathRecipient::new(move || {
            let mut dead = vm_died.0.lock().unwrap();
            *dead = true;
            vm_died.1.notify_all();
        })
    };
    vm.as_binder().link_to_death(&mut death_recipient)?;
    let _dead = vm_died.1.wait_while(vm_died.0.lock().unwrap(), |dead| !*dead).unwrap();
    println!("VM died");
    Ok(())
}
