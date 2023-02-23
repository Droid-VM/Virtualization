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

//! Main executable of RKP VM client.

use anyhow::Result;
use log::{error, info};
use std::{ffi::c_void, panic, ptr};
use vm_payload_bindgen::AVmPayload_generateCertificate;

/// Entry point of the RKP VM client.
#[allow(non_snake_case)]
#[no_mangle]
pub fn AVmPayload_main() {
    if let Err(e) = try_main() {
        error!("failed with {:?}", e);
        std::process::exit(1);
    }
}

fn try_main() -> Result<()> {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("rkpvm_client")
            .with_min_level(log::Level::Debug),
    );
    // Redirect panic messages to logcat.
    panic::set_hook(Box::new(|panic_info| {
        error!("{}", panic_info);
    }));
    info!("Welcome to RKP VM Client!");
    let csr = b"Hello from RKP VM";
    // SAFETY: TODO
    let certificate_size = unsafe {
        AVmPayload_generateCertificate(csr.as_ptr() as *const c_void, csr.len(), ptr::null_mut(), 0)
    };
    let mut certificate = vec![0u8; certificate_size];
    // SAFETY: TODO
    unsafe {
        AVmPayload_generateCertificate(
            csr.as_ptr() as *const c_void,
            csr.len(),
            certificate.as_mut_ptr() as *mut c_void,
            certificate.len(),
        );
    };
    info!("Certificate: {:?}", certificate);
    Ok(())
}
