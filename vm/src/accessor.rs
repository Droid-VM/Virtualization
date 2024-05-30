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

use crate::main::{Opt, VmService};
use android_os::aidl::android::os::{BnAccessor, IAccessorServer};
use android_system_virtualizationservice::{
    aidl::android::system::virtualizationservice::{
        VirtualMachineConfig::VirtualMachineConfig,
        VirtualMachinePayloadConfig::VirtualMachinePayloadConfig,
    },
    binder::{ParcelFileDescriptor, ProcessState},
};
use binder::{
    self, wait_for_interface, BinderFeatures, ExceptionCode, Interface, IntoBinderResult,
    LazyServiceGuard, ParcelFileDescriptor, Status, Strong,
};
use log::{debug, error, info};
use nix::sys::termios;
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::channel;

#[derive(Debug, Default)]
struct Accessor {
    vm: VmInstance,
    service_ports: HashMap<CString, u32>,
    /// Keeps our service process running as long as this VM context exists.
    _lazy_service_guard: LazyServiceGuard,
}

impl Accessor {
    // TODO: More protocol supports (e.g. TIPC)
    pub fn new_with_lazy_services(vm: VmInstance, services: &Vec<VmService>) -> Result<Self> {
        let service_ports: HashMap<_, _> =
            services.iter().map(|s| (s.service_name, s.service_port)).collect();
        let accessor = Self { vm, service_ports, ..Default::default() };

        for service in services {
            let accessor_binder =
                BnAccessor::new_binder(accessor.clone(), BinderFeatures::default());
            binder::register_lazy_service(&service.service_name, accessor.as_binder())
                .expect("Failed to register service");
        }

        Ok(accessor)
    }
}

impl Interface for Accessor {}

impl IAccessor for Accessor {
    fn connectToRpcSession(&self, service_name: &str) -> binder::Result<ParcelFileDescriptor> {
        let port = self
            .services
            .get(service_name())
            .expect("Unregistered service name, {service_name}, registered={}", self.services);
        self.vm.wait_until_ready(Duration::from_secs(10)).unwrap();

        info!("{service_name} is ready. Connecting to service");

        // Connect to service in the VM via connect_service();
        self.vm.connect_service(port);
    }
}
