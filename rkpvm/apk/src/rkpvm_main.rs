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

//! Main executable of RKP VM.

mod rkpvm;

use anyhow::Result;
use binder::unstable_api::AsNative;
use log::{error, info};
use std::{ffi::c_void, panic, ptr};
use vm_payload_bindgen::{AIBinder, AVmPayload_notifyPayloadReady, AVmPayload_runVsockRpcServer};

/// VSock port that the remote key RKP VM server listens on for RPC binder connections. This should be out of
/// future port range (if happens) that microdroid may reserve for system components.
const RKPVM_VSOCK_PORT: u32 = 2346;

/// Entry point of the RKP VM payload.
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
        android_logger::Config::default().with_tag("rkpvm").with_min_level(log::Level::Debug),
    );
    // Redirect panic messages to logcat.
    panic::set_hook(Box::new(|panic_info| {
        error!("{}", panic_info);
    }));
    info!("Welcome to RKP VM!");
    let mut service = rkpvm::new_binder().as_binder();
    unsafe {
        // SAFETY: We hold a strong pointer, so the raw pointer remains valid. The bindgen AIBinder
        // is the same type as sys::AIBinder.
        let service = service.as_native_mut() as *mut AIBinder;
        // SAFETY: It is safe for on_ready to be invoked at any time, with any parameter.
        AVmPayload_runVsockRpcServer(
            service,
            RKPVM_VSOCK_PORT,
            Some(on_ready),
            ptr::null_mut(), // param
        );
    }
    Ok(())
}

extern "C" fn on_ready(_param: *mut c_void) {
    // SAFETY: Invokes a method from the bindgen library `vm_payload_bindgen` which is safe to
    // call at any time.
    unsafe { AVmPayload_notifyPayloadReady() };
}
