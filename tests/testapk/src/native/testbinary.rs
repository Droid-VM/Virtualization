/*
 * Copyright 2024 The Android Open Source Project
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

//! A VM payload that exists to allow testing of the Rust wrapper for the VM payload APIs.

use anyhow::Result;
use log::{error, info};
use std::panic;

vm_payload::main!(main);

// Entry point of the Service VM client.
fn main() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("microdroid_testlib_rust")
            .with_max_level(log::LevelFilter::Debug),
    );
    // Redirect panic messages to logcat.
    panic::set_hook(Box::new(|panic_info| {
        error!("{}", panic_info);
    }));
    if let Err(e) = try_main() {
        error!("failed with {:?}", e);
        std::process::exit(1);
    }
}

fn try_main() -> Result<()> {
    info!("Welcome to the Rust test binary");

    //vm_payload::run_single_vsock_service(AttestationService::new_binder(), PORT.try_into()?)
    Ok(())
}
