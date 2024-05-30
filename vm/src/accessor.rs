// Copyright 2024, The Android Open Source Project
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

//! IAcessor implementation.
//! TODO: Keep this in proper places, so other pVMs can use this.
//! TODO: Allows to customize VMs for launching. (e.g. port, ...)

use android_os_accessor::aidl::android::os::IAccessor::IAccessor;
use binder::{self, Interface, LazyServiceGuard, ParcelFileDescriptor};
use log::info;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vmclient::VmInstance;

#[derive(Debug)]
pub struct Accessor {
    // Note: we can't simply keep reference by specifying lifetime to Accessor,
    //       because 'trait Interface' requires 'static.
    vm: Arc<Mutex<VmInstance>>,
    name: String,
    port: i32,
    /// Keeps our service process running as long as this VM context exists.
    _lazy_service_guard: LazyServiceGuard,
}

impl Accessor {
    pub fn new(vm: Arc<Mutex<VmInstance>>, name: String, port: i32) -> Self {
        Self { vm, name, port, _lazy_service_guard: Default::default() }
    }
}

impl Interface for Accessor {}

impl IAccessor for Accessor {
    fn connectToRpcSession(&self) -> binder::Result<ParcelFileDescriptor> {
        let vm = self.vm.lock().unwrap();
        vm.wait_until_ready(Duration::from_secs(10)).unwrap();

        info!("{} is ready. Connecting to service via port {}", self.name, self.port);

        vm.vm.connectVsock(self.port)
    }
}
