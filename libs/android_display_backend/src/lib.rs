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

extern crate nativewindow_bindgen as ffi;

use ffi::ANativeWindow;
use ffi::ANativeWindow_acquire;
use ffi::ANativeWindow_setBuffersGeometry;
use ffi::ANativeWindow_lock;
use ffi::ANativeWindow_unlockAndPost;
use ffi::ANativeWindow_Buffer;
use ffi::AHardwareBuffer_Format::*;

use std::ffi::c_char;
use std::ffi::CStr;
use crate::binder::binder_impl::Binder;
use libcrosvm_android_display_service::aidl::android::crosvm::ICrosvmAndroidDisplayService::BnCrosvmAndroidDisplayService;
use libcrosvm_android_display_service::aidl::android::crosvm::ICrosvmAndroidDisplayService::ICrosvmAndroidDisplayService;
use libcrosvm_android_display_service::binder::Strong;
use libcrosvm_android_display_service::binder;
use nativewindow::Surface;
use std::sync::Condvar;
use std::sync::Mutex;


/// Creates a context for the android display backend. A binder service is registered to the
/// service manager using the given name.
/// # Safety
/// `service_name` should be a non-null pointer to a utf-8 encoded string
/// `service_name_len` should be the length of the string
/// The returned context is created in the heap. The caller should not attempt to delete the
/// object by itself. When the context is no longer used, it should be deleted via
/// destroy_android_display_context.
#[no_mangle]
pub unsafe extern "C" fn create_android_display_context(
    service_name: *const c_char,
) -> *mut AndroidDisplayContext {
    let name = String::from_utf8_lossy(
        // SAFETY: service_name is of length service_name_len
        unsafe {
            CStr::from_ptr(service_name)
        }.to_bytes()
    );
    Box::leak(Box::new(AndroidDisplayContext::new(&name).unwrap()))
}

/// Destroys the given context object
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

/// Creates a window
/// # Safety
/// `ctx should be a non-null pointer obtained from create_android_display_context
#[no_mangle]
pub unsafe extern "C" fn create_android_surface(
    ctx: *mut AndroidDisplayContext,
    width: u32,
    height: u32,
) -> *mut ANativeWindow {
    // SAFETY: aaa
    let ctx = unsafe { ctx.as_ref() }.unwrap();
    let surface = ctx.get_surface();
    let ret = surface.0.as_ptr();
    // SAFETY: bbb
    unsafe {
        ANativeWindow_acquire(ret);
        ANativeWindow_setBuffersGeometry(
            ret,
            width.try_into().unwrap(),
            height.try_into().unwrap(),
            AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM.try_into().unwrap());
    }
    ret
}

/// Gets the pointer to the buffer
/// # Safety
/// `ctx` should be a non-null pointer obtained from create_android_display_context
#[no_mangle]
pub unsafe extern "C" fn get_android_surface_buffer(
    surface: *mut ANativeWindow,
) -> *mut u8 {
    let mut buffer = ANativeWindow_Buffer{
        width: 0,
        height: 0,
        stride: 0,
        format: 0,
        bits: std::ptr::null_mut(),
        reserved: [0; 6usize],
    };
    // SAFETY: ccc 
    unsafe {
        ANativeWindow_lock(surface, &mut buffer as *mut ANativeWindow_Buffer, std::ptr::null_mut())
    };
    buffer.bits as *mut u8
}

/// doc
/// # Safety
/// `ctx` should be a non-null pointer obtained from create_android_display_context
#[no_mangle]
pub unsafe extern "C" fn post_android_surface_buffer(
    surface: *mut ANativeWindow,
) {
    // SAFETY: aaa
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
    fn setSurface(&self, surface: &mut Surface) -> binder::Result<()> {
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

/// doc
pub struct AndroidDisplayContext {
    service: Strong<dyn ICrosvmAndroidDisplayService>,
}

impl AndroidDisplayContext {
    fn new(name: &str) -> binder::Result<Self> {
        let service = BnCrosvmAndroidDisplayService::new_binder(
            AndroidDisplayService::default(),
            binder::BinderFeatures::default(),
        );
        // TODO: switch to binder_rpc. Then name shall be the path of the UDS that the service
        // should listen to.
        binder::add_service(name, service.as_binder())?;
        binder::ProcessState::start_thread_pool();
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
