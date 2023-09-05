// Copyright 2023, The Android Open Source Project
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

//! This module regroups functions to ensure that at any given time there is
//! only one running Service VM.

use anyhow::{ensure, Result};
use lazy_static::lazy_static;
use log::info;
use service_vm_manager::ServiceVm;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

lazy_static! {
    static ref SERVICE_VM_STATE: State = State::default();
}

/// The running state of the Service VM.
#[derive(Debug, Default)]
struct State {
    is_running: Mutex<bool>,
    cvar: Condvar,
}

impl State {
    fn wait_until_no_service_vm_running(&self) -> Result<MutexGuard<'_, bool>> {
        let result = self
            .cvar
            .wait_timeout_while(
                self.is_running.lock().unwrap(),
                Duration::from_secs(10),
                |&mut is_running| is_running,
            )
            .unwrap();
        ensure!(
            !result.1.timed_out(),
            "Timed out while waiting for the running service VM to stop."
        );
        Ok(result.0)
    }

    fn is_running_guard(&self) -> MutexGuard<'_, bool> {
        self.is_running.lock().unwrap()
    }
}

/// Starts the service VM.
/// At any given time,  only one service should be running. If a service VM is
/// already running, this function will start the service VM once the running one
/// shuts down.
pub fn start() -> Result<ServiceVm> {
    let mut is_running_guard = SERVICE_VM_STATE.wait_until_no_service_vm_running()?;
    let vm = ServiceVm::start()?;
    *is_running_guard = true;
    Ok(vm)
}

/// Shuts down the given service VM instance.
pub fn shutdown(vm: ServiceVm) -> Result<()> {
    let mut is_running_guard = SERVICE_VM_STATE.is_running_guard();
    ensure!(*is_running_guard, "No running service VM to shut down.");

    drop(vm);
    info!("Shutdown the service VM successfully.");

    *is_running_guard = false;
    SERVICE_VM_STATE.cvar.notify_one();
    Ok(())
}
