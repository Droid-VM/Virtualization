// Copyright 2024, The Android Open Source Project
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

//! Crate implementing crosvm GPU display for Android

use anyhow::Result;
use nativewindow::Surface;
extern crate nativewindow_bindgen;
use nativewindow_bindgen::ANativeWindow;
use nativewindow_bindgen::ANativeWindow_acquire;
use nativewindow_bindgen::ANativeWindow_setBuffersGeometry;
use nativewindow_bindgen::ANativeWindow_lock;
use nativewindow_bindgen::ANativeWindow_unlockAndPost;
use nativewindow_bindgen::ANativeWindow_Buffer;
use nativewindow_bindgen::AHardwareBuffer_Format::*;

use crate::binder::binder_impl::Binder;
use libcrosvm_android_display_service::aidl::android::crosvm::ICrosvmAndroidDisplayService::BnCrosvmAndroidDisplayService;
use libcrosvm_android_display_service::aidl::android::crosvm::ICrosvmAndroidDisplayService::ICrosvmAndroidDisplayService;
use libcrosvm_android_display_service::binder::Strong;
use libcrosvm_android_display_service::binder;
use rpcbinder::RpcServer;
use rpcbinder::FileDescriptorTransportMode;
use std::ffi::CStr;
use std::ffi::c_char;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::Condvar;
use std::sync::Mutex;
use std::os::unix::ffi::OsStrExt;
use std::ffi::OsStr;

/// Creates a context for the android display backend. A binder service is created and is listening
/// on the UNIX domain socket at the path `uds_path`.///
///
/// # Safety
/// * `uds_path` should be a non-null pointer to the pyath to the UDS.
/// * The returned context is created in the heap. The caller should not attempt to delete the
/// object by itself. When the context is no longer used, it should be deleted via
/// destroy_android_display_context.
#[no_mangle]
pub unsafe extern "C" fn create_android_display_context(
    uds_path: *const c_char,
) -> *mut AndroidDisplayContext {
    // SAFETY: uds_path is a valid null-terminated string
    let uds_path = unsafe { CStr::from_ptr(uds_path)};
    let uds_path = OsStr::from_bytes(uds_path.to_bytes()).as_ref();
    let ctx = Box::new(AndroidDisplayContext::new(uds_path).unwrap());

    // Intentional leak. This is deleted by client calling destroy_android_display_context.
    Box::leak(ctx)
}

/// Destroys the given context object
///
/// # Safety
/// `ctx` should be a non-null pointer obtained from create_android_display_context
#[no_mangle]
pub unsafe extern "C" fn destroy_android_display_context(
    ctx: *mut AndroidDisplayContext,
) {
    // SAFETY: ctx is returned from create_android_display_context
    let _ = unsafe {
        Box::from_raw(ctx)
    };
}

/// Creates an Android-side window f the specific width and height.
///
/// # Safety
/// `ctx` should be a non-null pointer obtained from create_android_display_context
/// Returned `ANativeWindow` is an opaque handle to the created window.
#[no_mangle]
pub unsafe extern "C" fn create_android_surface(
    ctx: *mut AndroidDisplayContext,
    width: u32,
    height: u32,
) -> *mut ANativeWindow {
    // SAFETY:  `ctx` is a valid non-null pointer created by create_android_display_context
    let ctx = unsafe { ctx.as_ref() }.unwrap();

    let mut surface = ctx.get_surface();
    let window  = &mut surface as *mut Surface as *mut ANativeWindow;

    // SAFETY: `window` is an opaque handle
    unsafe {
        ANativeWindow_acquire(window);
        ANativeWindow_setBuffersGeometry(
            window,
            width.try_into().unwrap(),
            height.try_into().unwrap(),
            AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM.try_into().unwrap());
    }
    window
}

/// Gets the pointer to the buffer. The caller (crosvm) has exclusive access to the buffer.
/// 
/// # Safety
/// `ctx` should be a non-null pointer obtained from create_android_display_context
#[no_mangle]
pub unsafe extern "C" fn get_android_surface_buffer(
    surface: *mut ANativeWindow,
) -> *mut u8 {
    // TODO: derive Default for ANativeWindow_Buffer?
    let mut buffer = ANativeWindow_Buffer{
        width: 0,
        height: 0,
        stride: 0,
        format: 0,
        bits: std::ptr::null_mut(),
        reserved: [0; 6usize],
    };

    // SAFETY: surface is an opaque handle. And the buffer struct can be dropped after this
    // function returns because it's simply an out parameter. The real buffer is not leaked outside
    // of the lock function.
    unsafe {
        ANativeWindow_lock(surface, &mut buffer as *mut ANativeWindow_Buffer, std::ptr::null_mut())
    };
    buffer.bits as *mut u8
}

/// Gives the buffer back to Android for displaying.
///
/// # Safety
/// `ctx` should be a non-null pointer obtained from create_android_display_context
#[no_mangle]
pub unsafe extern "C" fn post_android_surface_buffer(
    surface: *mut ANativeWindow,
) {
    // SAFETY: surface is an opaque handle.
    unsafe {
        ANativeWindow_unlockAndPost(surface)
    };
}

#[derive(Default)]
struct AndroidDisplayService {
    // TODO: support at least two surfaces: one for regular scanout and the other for cursor
    surface: Mutex<Option<Surface>>,
    surface_set: Condvar,
}

impl binder::Interface for AndroidDisplayService {}

impl ICrosvmAndroidDisplayService for AndroidDisplayService {
    fn setSurface(&self, surface: &Surface) -> binder::Result<()> {
        let mut s = self.surface.lock().unwrap();
        *s = Some(surface.clone());
        self.surface_set.notify_one();
        Ok(())
    }

    fn removeSurface(&self) -> binder::Result<()> {
        let mut s = self.surface.lock().unwrap();
        *s = None;
        self.surface_set.notify_one();
        Ok(())
    }
}

/// `AndroidDisplayContext` is a context object that holds other objects implementing the Android
/// display backend
pub struct AndroidDisplayContext {
    service: Strong<dyn ICrosvmAndroidDisplayService>,
}

impl AndroidDisplayContext {
    fn new(uds_path: &Path) -> Result<Self> {
        let service = BnCrosvmAndroidDisplayService::new_binder(
            AndroidDisplayService::default(),
            binder::BinderFeatures::default(),
        );

        let (conn, _) = UnixListener::bind(uds_path)?.accept()?;
        let server = RpcServer::new_unix_domain_bootstrap(service.as_binder(), conn.into())?;
        server.set_supported_file_descriptor_transport_modes(&[FileDescriptorTransportMode::Unix]);
        std::thread::spawn(move|| server.join());

        Ok(Self{service})
    }

    // Wait until Surface is set and return a copy of it.
    fn get_surface(&self) -> Surface {
        let binder: Binder<BnCrosvmAndroidDisplayService> = self.service.as_binder().try_into().unwrap();
        let service = binder.downcast_binder::<AndroidDisplayService>().unwrap();
        let surface = service.surface_set.wait_while(
            service.surface.lock().unwrap(),
            |surface| surface.is_some()).unwrap();
        surface.as_ref().unwrap().clone()
    }
}
