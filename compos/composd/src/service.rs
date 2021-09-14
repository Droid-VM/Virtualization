/*
 * Copyright (C) 2021 The Android Open Source Project
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

//! Implementation of IIsolatedCompilationService, called from system server when compilation is
//! desired.

use crate::compos_instance::CompOsInstance;
use android_system_composd::aidl::android::system::composd::IIsolatedCompilationService::{
    BnIsolatedCompilationService, IIsolatedCompilationService,
};
use android_system_composd::binder::{self, BinderFeatures, Interface, Status, Strong};
use anyhow::{bail, Context, Result};
use log::{error, info, warn};
use std::ffi::CString;
use std::process::{Command, Stdio};

const ODREFRESH_BIN: &str = "/apex/com.android.art/bin/odrefresh";
const COMPILATION_SUCCESS: i32 = 80;

pub struct IsolatedCompilationService {}

pub fn new_binder() -> Strong<dyn IIsolatedCompilationService> {
    let service = IsolatedCompilationService {};
    BnIsolatedCompilationService::new_binder(service, BinderFeatures::default())
}

impl Interface for IsolatedCompilationService {}

impl IIsolatedCompilationService for IsolatedCompilationService {
    fn runForcedCompile(&self) -> binder::Result<()> {
        to_binder_result(self.do_run_forced_compile())
    }
}

fn to_binder_result<T>(result: Result<T>) -> binder::Result<T> {
    result.map_err(|e| {
        error!("{:#}", e);
        let message = CString::new(format!("{:#}", e)).unwrap();
        Status::new_service_specific_error(-1, Some(&message))
    })
}

impl IsolatedCompilationService {
    fn do_run_forced_compile(&self) -> Result<()> {
        info!("runForcedCompile");

        // TODO: Create instance if need be, handle instance failure, prevent
        // multiple instances running
        let comp_os = CompOsInstance::start_current_instance().context("Starting CompOS")?;

        // TODO: Move odrefresh out into its own module
        let odrefresh = Command::new(ODREFRESH_BIN)
            .arg(format!("--use-compilation-os={}", comp_os.cid()))
            .arg("--force-compile")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Running odrefresh")?;

        // TODO: timeout? Log the output?
        let output = odrefresh.wait_with_output()?;
        if let Some(COMPILATION_SUCCESS) = output.status.code() {
            Ok(())
        } else {
            warn!("stdout {}", String::from_utf8_lossy(&output.stdout));
            warn!("stderr {}", String::from_utf8_lossy(&output.stderr));
            bail!("odrefresh exited with {}", output.status)
        }
    }
}
