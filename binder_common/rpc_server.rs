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

//! Helpers for implementing an RPC Binder server.

use binder::public_api::SpIBinder;
use binder::unstable_api::AsNative;
use std::os::raw;

/// Docs go here
pub fn run_rpc_server<F>(mut service: SpIBinder, port: u32, on_ready: F) -> bool
where
    F: FnOnce(),
{
    let service = service.as_native_mut() as *mut binder_rpc_unstable_bindgen::AIBinder;

    let mut ready_notifier = ReadyNotifier(Some(on_ready));

    // SAFETY: Service ownership is transferring to the server and won't be valid afterward.
    // Plus the binder objects are threadsafe.
    // RunRpcServerCallback does not retain a reference to ready_callback, and only ever
    // calls it with the param we provide during the lifetime of ready_notifier.
    unsafe {
        binder_rpc_unstable_bindgen::RunRpcServerCallback(
            service,
            port,
            Some(ReadyNotifier::<F>::ready_callback),
            ready_notifier.as_void_ptr(),
        )
    }
}

struct ReadyNotifier<F>(Option<F>)
where
    F: FnOnce();

impl<F> ReadyNotifier<F>
where
    F: FnOnce(),
{
    fn notify(&mut self) {
        if let Some(on_ready) = self.0.take() {
            on_ready();
        }
    }

    fn as_void_ptr(&mut self) -> *mut raw::c_void {
        self as *mut _ as *mut raw::c_void
    }

    unsafe extern "C" fn ready_callback(param: *mut raw::c_void) {
        // SAFETY: This is only ever called by RunRpcServerCallback, within the lifetime of the
        // ReadyNotifier, with param taking the value returned by as_void_ptr (so a properly aligned
        // non-null pointer to an initialized instance).
        let ready_notifier = param as *mut Self;
        ready_notifier.as_mut().unwrap().notify()
    }
}
