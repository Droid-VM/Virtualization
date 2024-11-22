// Copyright 2024 The Android Open Source Project
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

//! Stable C library for AVF.

use std::ffi::CStr;
use std::fs::File;
use std::os::fd::FromRawFd;
use std::os::raw::{c_char, c_int};
use std::ptr;

use android_system_virtualizationservice::{
    aidl::android::system::virtualizationservice::{
        DiskImage::DiskImage, IVirtualizationService::IVirtualizationService,
        VirtualMachineConfig::VirtualMachineConfig,
        VirtualMachineRawConfig::VirtualMachineRawConfig,
    },
    binder::{ParcelFileDescriptor, Strong},
};
use avf_bindgen::StopReason;
use nix::errno::Errno;
use vmclient::{DeathReason, VirtualizationService, VmInstance};

/// Create a new virtual machine config object with no properties.
#[no_mangle]
pub extern "C" fn AVirtualMachineConfig_createRaw() -> *mut VirtualMachineConfig {
    let config = Box::new(VirtualMachineConfig::RawConfig(VirtualMachineRawConfig {
        platformVersion: "~1.0".to_owned(),
        ..Default::default()
    }));
    Box::into_raw(config)
}

/// Destroy a virtual machine config object.
///
/// # Safety
/// `config` must be a pointer returned by `AVirtualMachineConfig_create`. `config` must not be
/// used after deletion.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachineConfig_free(config: *mut VirtualMachineConfig) {
    if !config.is_null() {
        // SAFETY: `config` is assumed to be a valid, non-null pointer returned by
        // AVirtualMachineConfig_create. It's the only reference to the object.
        unsafe {
            let _ = Box::from_raw(config);
        }
    }
}

/// Set a name of a virtual machine.
///
/// # Safety
/// `config` must be a pointer returned by `AVirtualMachineConfig_create`.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachineConfig_setName(
    config: *mut VirtualMachineConfig,
    name: *const c_char,
) -> c_int {
    // SAFETY: `config` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachineConfig_create. It's the only reference to the object.
    match unsafe { &mut *config } {
        VirtualMachineConfig::RawConfig(config) => {
            // SAFETY: `name` is assumed to be a pointer to a valid C string.
            config.name = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();
            0
        }
        // AppConfig not supported yet
        _ => -(Errno::EINVAL as c_int),
    }
}

/// Set an instance ID of a virtual machine.
///
/// # Safety
/// `config` must be a pointer returned by `AVirtualMachineConfig_create`. `instanceId` must be a
/// valid, non-null pointer to 64-byte data.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachineConfig_setInstanceId(
    config: *mut VirtualMachineConfig,
    instance_id: *const u8,
) -> c_int {
    // SAFETY: `config` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachineConfig_create. It's the only reference to the object.
    match unsafe { &mut *config } {
        VirtualMachineConfig::RawConfig(config) => {
            // SAFETY: `instanceId` is assumed to be a valid pointer to 64 bytes of memory. `config`
            // is assumed to be a valid object returned by AVirtuaMachineConfig_create.
            // Both never overlap.
            unsafe {
                ptr::copy_nonoverlapping(instance_id, config.instanceId.as_mut_ptr(), 64);
            }
            0
        }
        // AppConfig not supported yet
        _ => -(Errno::EINVAL as c_int),
    }
}

/// Set a kernel image of a virtual machine.
///
/// # Safety
/// `config` must be a pointer returned by `AVirtualMachineConfig_create`. `fd` must be a valid
/// file descriptor or -1. `AVirtualMachineConfig_setKernel` takes ownership of `fd` and `fd` will
/// be closed upon `AVirtualMachineConfig_delete`.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachineConfig_setKernel(
    config: *mut VirtualMachineConfig,
    fd: c_int,
) -> c_int {
    // SAFETY: `config` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachineConfig_create. It's the only reference to the object.
    match unsafe { &mut *config } {
        VirtualMachineConfig::RawConfig(config) => {
            config.kernel = get_file_from_fd(fd).map(ParcelFileDescriptor::new);
            0
        }
        // AppConfig not supported yet
        _ => -(Errno::EINVAL as c_int),
    }
}

/// Set an init rd of a virtual machine.
///
/// # Safety
/// `config` must be a pointer returned by `AVirtualMachineConfig_create`. `fd` must be a valid
/// file descriptor or -1. `AVirtualMachineConfig_setInitRd` takes ownership of `fd` and `fd` will
/// be closed upon `AVirtualMachineConfig_delete`.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachineConfig_setInitRd(
    config: *mut VirtualMachineConfig,
    fd: c_int,
) -> c_int {
    // SAFETY: `config` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachineConfig_create. It's the only reference to the object.
    match unsafe { &mut *config } {
        VirtualMachineConfig::RawConfig(config) => {
            config.initrd = get_file_from_fd(fd).map(ParcelFileDescriptor::new);
            0
        }
        // AppConfig not supported yet
        _ => -(Errno::EINVAL as c_int),
    }
}

/// Add a disk for a virtual machine.
///
/// # Safety
/// `config` must be a pointer returned by `AVirtualMachineConfig_create`. `fd` must be a valid
/// file descriptor. `AVirtualMachineConfig_addDisk` takes ownership of `fd` and `fd` will be
/// closed upon `AVirtualMachineConfig_delete`.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachineConfig_addDisk(
    config: *mut VirtualMachineConfig,
    fd: c_int,
    writable: bool,
) -> c_int {
    // SAFETY: `config` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachineConfig_create. It's the only reference to the object.
    match unsafe { &mut *config } {
        VirtualMachineConfig::RawConfig(config) => {
            match get_file_from_fd(fd) {
                // partition not supported yet
                None => -1,
                Some(file) => {
                    config.disks.push(DiskImage {
                        image: Some(ParcelFileDescriptor::new(file)),
                        writable,
                        ..Default::default()
                    });
                    0
                }
            }
        }
        // AppConfig not supported yet
        _ => -(Errno::EINVAL as c_int),
    }
}

/// Set how much memory will be given to a virtual machine.
///
/// # Safety
/// `config` must be a pointer returned by `AVirtualMachineConfig_create`.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachineConfig_setMemoryMib(
    config: *mut VirtualMachineConfig,
    memory_mib: i32,
) -> c_int {
    // SAFETY: `config` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachineConfig_create. It's the only reference to the object.
    match unsafe { &mut *config } {
        VirtualMachineConfig::RawConfig(config) => {
            config.memoryMib = memory_mib;
            0
        }
        // AppConfig not supported yet
        _ => -(Errno::EINVAL as c_int),
    }
}

/// NOT IMPLEMENTED. Always returns -ENOTSUP
#[no_mangle]
pub extern "C" fn AVirtualMachineConfig_setDeviceTreeOverlay(
    _config: *mut VirtualMachineConfig,
    _path: *const c_char,
) -> c_int {
    -(Errno::ENOTSUP as c_int)
}

/// Set whether a virtual machine is protected or not.
///
/// # Safety
/// `config` must be a pointer returned by `AVirtualMachineConfig_create`.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachineConfig_setProtectedVm(
    config: *mut VirtualMachineConfig,
    protected_vm: bool,
) -> c_int {
    // SAFETY: `config` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachineConfig_create. It's the only reference to the object.
    match unsafe { &mut *config } {
        VirtualMachineConfig::RawConfig(config) => {
            config.protectedVm = protected_vm;
            0
        }
        // AppConfig not supported yet
        _ => -(Errno::EINVAL as c_int),
    }
}

/// Set whether a virtual machine uses memory ballooning or not.
///
/// # Safety
/// `config` must be a pointer returned by `AVirtualMachineConfig_create`.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachineConfig_setBalloon(
    config: *mut VirtualMachineConfig,
    balloon: bool,
) -> c_int {
    // SAFETY: `config` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachineConfig_create. It's the only reference to the object.
    match unsafe { &mut *config } {
        VirtualMachineConfig::RawConfig(config) => {
            config.noBalloon = !balloon;
            0
        }
        // AppConfig not supported yet
        _ => -(Errno::EINVAL as c_int),
    }
}

/// NOT IMPLEMENTED.
///
/// # Returns
/// It always returns `-ENOTSUP`.
#[no_mangle]
pub extern "C" fn AVirtualMachineConfig_setHypervisorSpecificAuthMethod(
    _config: *mut VirtualMachineConfig,
    _enable: bool,
) -> c_int {
    -(Errno::ENOTSUP as c_int)
}

/// NOT IMPLEMENTED.
///
/// # Returns
/// It always returns `-ENOTSUP`.
#[no_mangle]
pub extern "C" fn AVirtualMachineConfig_addCustomMemoryBackingFile(
    _config: *mut VirtualMachineConfig,
    _fd: c_int,
    _range_start: usize,
    _range_end: usize,
) -> c_int {
    -(Errno::ENOTSUP as c_int)
}

/// NOT IMPLEMENTED.
///
/// # Returns
/// It always returns `-ENOTSUP`.
#[no_mangle]
pub extern "C" fn AVirtualMachineConfig_addReservedMmioRange(
    _config: *mut VirtualMachineConfig,
    _range_start: usize,
    _range_end: usize,
) -> c_int {
    -(Errno::ENOTSUP as c_int)
}

/// Spawn a new instance of `virtmgr`, a child process that will host the `VirtualizationService`
/// AIDL service, and connect to the child process.
///
/// # Safety
/// `service_ptr` must be a valid, non-null pointer to a mutable raw pointer.
#[no_mangle]
pub unsafe extern "C" fn AVirtualizationService_create(
    service_ptr: *mut *mut Strong<dyn IVirtualizationService>,
    early: bool,
) -> c_int {
    let virtmgr =
        if early { VirtualizationService::new_early() } else { VirtualizationService::new() };
    match virtmgr {
        Ok(virtmgr) => match virtmgr.connect() {
            Ok(service) => {
                // SAFETY: `service` is assumed to be a valid, non-null pointer to a mutable raw
                // pointer. `service` is the only reference here and `config` takes
                // ownership.
                unsafe {
                    *service_ptr = Box::into_raw(Box::new(service));
                }
                0
            }
            Err(_) => -(Errno::ECONNREFUSED as c_int),
        },
        Err(e) => match e.raw_os_error() {
            Some(os_err) => -os_err,
            None => -(Errno::EIO as c_int),
        },
    }
}

/// Destroy a VirtualizationService object.
///
/// # Safety
/// `service` must be a pointer returned by `AVirtualizationService_create` or
/// `AVirtualizationService_create_early`. `service` must not be reused after deletion.
#[no_mangle]
pub unsafe extern "C" fn AVirtualizationService_free(
    service: *mut Strong<dyn IVirtualizationService>,
) {
    if !service.is_null() {
        // SAFETY: `service` is assumed to be a valid, non-null pointer returned by
        // `AVirtualizationService_create` or `AVirtualizationService_create_early`. It's the only
        // reference to the object.
        unsafe {
            let _ = Box::from_raw(service);
        }
    }
}

/// Create a virtual machine with given `config`.
///
/// # Safety
/// `config` must be a pointer returned by `AVirtualMachineConfig_create`. `service` must be a
/// pointer returned by `AVirtualMachineConfig_create`. `vm_ptr` must be a valid, non-null pointer
/// to a mutable raw pointer. `console_out_fd`, `console_in_fd`, and `log_fd` must be a valid file
/// descriptor or -1. `AVirtualMachine_create` takes ownership of `console_out_fd`, `console_in_fd`,
/// and `log_fd`, and taken file descriptors must not be reused.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachine_create(
    service: *const Strong<dyn IVirtualizationService>,
    config: *const VirtualMachineConfig,
    console_out_fd: c_int,
    console_in_fd: c_int,
    log_fd: c_int,
    vm_ptr: *mut *mut VmInstance,
) -> c_int {
    // SAFETY: `service` is assumed to be a valid, non-null pointer returned by
    // `AVirtualizationService_create` or `AVirtualizationService_create_early`. It's the only
    // reference to the object.
    let service = unsafe { &*service };

    // SAFETY: `config` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachineConfig_create. It's the only reference to the object.
    let config = unsafe { &*config };

    let console_out = get_file_from_fd(console_out_fd);
    let console_in = get_file_from_fd(console_in_fd);
    let log = get_file_from_fd(log_fd);

    match VmInstance::create(service.as_ref(), config, console_out, console_in, log, None, None) {
        Ok(vm) => {
            // SAFETY: `vm_ptr` is assumed to be a valid, non-null pointer to a mutable raw pointer.
            // `vm` is the only reference here and `vm_ptr` takes ownership.
            unsafe {
                *vm_ptr = Box::into_raw(Box::new(vm));
            }
            0
        }
        Err(_) => -(Errno::EIO as c_int),
    }
}

/// Start a virtual machine.
///
/// # Safety
/// `vm` must be a pointer returned by `AVirtualMachine_create`.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachine_start(vm: *const VmInstance) -> c_int {
    // SAFETY: `vm` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachine_create. It's the only reference to the object.
    let vm = unsafe { &*vm };
    match vm.start() {
        Ok(_) => 0,
        Err(_) => -(Errno::EIO as c_int),
    }
}

/// Stop a virtual machine.
///
/// # Safety
/// `vm` must be a pointer returned by `AVirtualMachine_create`.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachine_stop(vm: *const VmInstance) -> c_int {
    // SAFETY: `vm` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachine_create. It's the only reference to the object.
    let vm = unsafe { &*vm };
    match vm.stop() {
        Ok(_) => 0,
        Err(_) => -(Errno::EIO as c_int),
    }
}

/// Wait until a virtual machine stops.
///
/// # Safety
/// `vm` must be a pointer returned by `AVirtualMachine_create`.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachine_waitForStop(vm: *const VmInstance) -> StopReason {
    // SAFETY: `vm` is assumed to be a valid, non-null pointer returned by
    // AVirtualMachine_create. It's the only reference to the object.
    let vm = unsafe { &*vm };
    match vm.wait_for_death() {
        DeathReason::VirtualizationServiceDied => StopReason::VIRTUALIZATION_SERVICE_DIED,
        DeathReason::InfrastructureError => StopReason::INFRASTRUCTURE_ERROR,
        DeathReason::Killed => StopReason::KILLED,
        DeathReason::Unknown => StopReason::UNKNOWN,
        DeathReason::Shutdown => StopReason::SHUTDOWN,
        DeathReason::StartFailed => StopReason::START_FAILED,
        DeathReason::Reboot => StopReason::REBOOT,
        DeathReason::Crash => StopReason::CRASH,
        DeathReason::PvmFirmwarePublicKeyMismatch => StopReason::PVM_FIRMWARE_PUBLIC_KEY_MISMATCH,
        DeathReason::PvmFirmwareInstanceImageChanged => {
            StopReason::PVM_FIRMWARE_INSTANCE_IMAGE_CHANGED
        }
        DeathReason::MicrodroidFailedToConnectToVirtualizationService => {
            StopReason::MICRODROID_FAILED_TO_CONNECT_TO_VIRTUALIZATION_SERVICE
        }
        DeathReason::MicrodroidPayloadHasChanged => StopReason::MICRODROID_PAYLOAD_HAS_CHANGED,
        DeathReason::MicrodroidPayloadVerificationFailed => {
            StopReason::MICRODROID_PAYLOAD_VERIFICATION_FAILED
        }
        DeathReason::MicrodroidInvalidPayloadConfig => {
            StopReason::MICRODROID_INVALID_PAYLOAD_CONFIG
        }
        DeathReason::MicrodroidUnknownRuntimeError => StopReason::MICRODROID_UNKNOWN_RUNTIME_ERROR,
        DeathReason::Hangup => StopReason::HANGUP,
        DeathReason::Unrecognised(_) => StopReason::UNRECOGNISED,
    }
}

/// Destroy a virtual machine.
///
/// # Safety
/// `vm` must be a pointer returned by `AVirtualMachine_create`. `vm` must not be reused after
/// deletion.
#[no_mangle]
pub unsafe extern "C" fn AVirtualMachine_free(vm: *mut VmInstance) {
    if !vm.is_null() {
        // SAFETY: `vm` is assumed to be a valid, non-null pointer returned by
        // AVirtualMachine_create. It's the only reference to the object.
        unsafe {
            let _ = Box::from_raw(vm);
        }
    }
}

fn get_file_from_fd(fd: i32) -> Option<File> {
    if fd == -1 {
        None
    } else {
        // SAFETY: transferring ownership of `fd` from the caller
        Some(unsafe { File::from_raw_fd(fd) })
    }
}
