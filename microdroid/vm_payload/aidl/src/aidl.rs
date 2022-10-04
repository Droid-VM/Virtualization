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

//! Implementation of the AIDL interface of the android.system.vm_payload.

use android_system_vm_payload::aidl::android::system::vm_payload::IVirtualMachinePayload::IVirtualMachinePayload;
use android_system_virtualmachineservice::aidl::android::system::virtualmachineservice::IVirtualMachineService::IVirtualMachineService;
use binder::{Interface, Strong};

/// Implementation of `IVirtualMachinePayload`, the entry point of the AIDL service.
#[derive(Debug)]
pub struct VirtualMachinePayload {
    virtual_machine_service: Strong<dyn IVirtualMachineService>,
}

impl IVirtualMachinePayload for VirtualMachinePayload {
    fn notifyPayloadReady(&self) -> binder::Result<()> {
        self.virtual_machine_service.notifyPayloadReady()
    }
}

impl Interface for VirtualMachinePayload {}

impl VirtualMachinePayload {
    /// Creates a new `VirtualMachinePayload` instance from the `IVirtualMachineService` reference.
    pub fn new(vm_service: Strong<dyn IVirtualMachineService>) -> Self {
        Self { virtual_machine_service: vm_service }
    }
}
