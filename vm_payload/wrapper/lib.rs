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

mod attestation;

pub use attestation::{
    request_attestation, request_attestation_for_testing, AttestationError, AttestationResult,
};
use binder::unstable_api::AsNative;
use binder::{FromIBinder, Strong};
use std::ffi::{c_void, CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use vm_payload_bindgen::{
    AIBinder, AVmPayload_getApkContentsPath, AVmPayload_getEncryptedStoragePath,
    AVmPayload_getVmInstanceSecret, AVmPayload_notifyPayloadReady, AVmPayload_runVsockRpcServer,
};

/// Marks the main function of the VM payload.
///
/// When the VM is run, this function is called. If it returns, the VM ends.
///
/// Example:
///
/// ```rust
/// use log::info;
///
/// vm_payload::main!(vm_main);
///
/// fn vm_main() {
///     android_logger::init_once(
///          android_logger::Config::default()
///             .with_tag("example_vm_payload")
///             .with_max_level(log::LevelFilter::Info),
///     );
///     info!("Hello world");
/// }
/// ```
#[macro_export]
macro_rules! main {
    ($name:path) => {
        // Export a symbol with a name matching the extern declaration below.
        #[export_name = "rust_main"]
        fn __main() {
            // Ensure that the main function provided by the application has the correct type.
            $name()
        }
    };
}

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

/// TODO
pub fn apk_contents_path() -> &'static Path {
    // SAFETY: AVmPayload_getApkContentsPath always returns a non-null pointer to a
    // nul-terminated C string with static lifetime.
    let c_str = unsafe { CStr::from_ptr(AVmPayload_getApkContentsPath()) };
    Path::new(OsStr::from_bytes(c_str.to_bytes()))
}

/// TODO
pub fn encrypted_storage_path() -> Option<&'static Path> {
    // SAFETY: AVmPayload_getEncryptedStoragePath returns either null or a pointer to a
    // nul-terminated C string with static lifetime.
    let ptr = unsafe { AVmPayload_getEncryptedStoragePath() };
    if ptr.is_null() {
        None
    } else {
        // SAFETY: We know the pointer is not null, and so it is a valid C string.
        let c_str = unsafe { CStr::from_ptr(ptr) };
        Some(Path::new(OsStr::from_bytes(c_str.to_bytes())))
    }
}

/// TODO
pub fn get_vm_instance_secret<const N: usize>(identifier: &[u8], secret: &mut [u8]) {
    let secret_size = secret.len();
    assert!((1..=32).contains(&secret_size), "VM instance secrets can be up to 32 bytes long");

    // SAFETY: The function only reads from `[identifier]` within its bounds, and only writes to
    // `[secret]` within its bounds. Neither reference is retained, and we know neither is null.
    unsafe {
        AVmPayload_getVmInstanceSecret(
            identifier.as_ptr() as *const c_void,
            identifier.len(),
            secret.as_mut_ptr() as *mut c_void,
            secret_size,
        )
    }
}

// This is the real C entry point for the VM; we just forward to the Rust entry point.
#[allow(non_snake_case)]
#[no_mangle]
extern "C" fn AVmPayload_main() {
    extern "Rust" {
        fn rust_main();
    }

    // SAFETY: rust_main is provided by the application using the `main!` macro above, which makes
    // sure it has the right type.
    unsafe { rust_main() }
}
