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

use anyhow::Result;
use log::{debug, error};
use std::panic;

const RKPVM_SERVICE_NAME: &str = "android.virt.rkpvm";

fn main() {
    if let Err(e) = try_main() {
        error!("failed with {:?}", e);
        std::process::exit(1);
    }
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

    debug!("rkpvm is starting as a rpc service.");
    binder::ProcessState::start_thread_pool();
    binder::add_service(RKPVM_SERVICE_NAME, service).unwrap_or_else(|e| {
        panic!("Failed to register service {} because of {:?}.", RKPVM_SERVICE_NAME, e);
    });
    debug!("Joining thread pool.");
    binder::ProcessState::join_thread_pool();
    Ok(())
}
