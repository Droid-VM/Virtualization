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

//! This module contains the functions that interact with the Service VM
//! manager and ensure that at any given time only one service VM is running.

use anyhow::Result;
use service_vm_manager::{self, ServiceVm};

/// Starts the service VM and returns its instance.
/// The same instance image is used for different VMs.
/// TODO(b/278858244): Allow only one service VM running at each time.
pub fn start() -> Result<ServiceVm> {
    service_vm_manager::start()
}
