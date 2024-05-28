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

//! TODO

use binder::unstable_api::AsNative;
use binder::{FromIBinder, Strong};
use std::os::raw::c_void;
use std::ptr;
#[allow(unused_imports)] // TODO
use vm_payload_bindgen::{
    AIBinder, AVmAttestationResult, AVmAttestationResult_free,
    AVmAttestationResult_getCertificateAt, AVmAttestationResult_getCertificateCount,
    AVmAttestationResult_getPrivateKey, AVmAttestationResult_sign, AVmAttestationStatus,
    AVmAttestationStatus_toString, AVmPayload_notifyPayloadReady, AVmPayload_requestAttestation,
    AVmPayload_requestAttestationForTesting, AVmPayload_runVsockRpcServer,
};

/// TODO
pub fn notify_payload_ready() {
    // SAFETY: Invokes a method from the bindgen library `vm_payload_bindgen` which is safe to
    // call at any time.
    unsafe { AVmPayload_notifyPayloadReady() };
}

/// TODO
pub fn run_vsock_service<T>(service: Strong<T>, port: u32) -> !
where
    T: FromIBinder + ?Sized,
{
    extern "C" fn on_ready(_param: *mut c_void) {
        notify_payload_ready();
    }

    let mut service = service.as_binder();
    // The cast here is needed because the compiler doesn't know that our vm_payload_bindgen
    // AIBinder is the same type as binder_ndk_sys::AIBinder.
    let service = service.as_native_mut() as *mut AIBinder;
    let param = ptr::null_mut();
    // SAFETY: We have a strong reference to the service, so the raw pointer remains valid. It is
    // safe for on_ready to be invoked at any time, with any parameter.
    unsafe { AVmPayload_runVsockRpcServer(service, port, Some(on_ready), param) }
}
