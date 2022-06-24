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

//! An RPK service VM for VMs.

use anyhow::Result;
use log::debug;

use rkpvm_aidl_interface::aidl::com::android::rkpvm::IRkpVmService::{
    BnRkpVmService, IRkpVmService,
};
use rkpvm_aidl_interface::binder::{BinderFeatures, Interface, Result as BinderResult, Strong};

/// Constructs a binder object that implements IRkpVmService.
pub fn new_binder() -> Result<Strong<dyn IRkpVmService>> {
    let service = RkpVmService {};
    Ok(BnRkpVmService::new_binder(service, BinderFeatures::default()))
}

struct RkpVmService {}

impl Interface for RkpVmService {}

impl IRkpVmService for RkpVmService {
    fn placeholder(&self) -> BinderResult<()> {
        debug!("Ding dong!");
        Ok(())
    }
}
