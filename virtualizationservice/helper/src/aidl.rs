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

//! Implementation of the AIDL interface of the VirtualizationService.

use android_system_virtualizationservice_internal::aidl::android::system::virtualizationservice_internal::IVirtualizationServiceHelper::IVirtualizationServiceHelper;
use android_system_virtualizationservice_internal::binder::ParcelFileDescriptor;
use binder::{self, ExceptionCode, Interface, Status};
use lazy_static::lazy_static;
use std::fs::{read_link, File, OpenOptions};
use std::io::Write;
use std::os::fd::{FromRawFd};
use std::path::Path;
use nix::fcntl::OFlag;
use nix::unistd::pipe2;

pub const BINDER_SERVICE_IDENTIFIER: &str = "android.system.virtualizationservicehelper";

#[derive(Debug, Default)]
pub struct VirtualizationServiceHelper {}

impl VirtualizationServiceHelper {
    pub fn init() -> VirtualizationServiceHelper {
        VirtualizationServiceHelper::default()
    }
}

impl Interface for VirtualizationServiceHelper {}

impl IVirtualizationServiceHelper for VirtualizationServiceHelper {
    fn bindDevicesToVfioDriver(&self, devices: &[String]) -> binder::Result<ParcelFileDescriptor> {
        // permission check is already done by IVirtualizationServiceInternal.
        if !*IS_VFIO_SUPPORTED {
            return Err(Status::new_exception_str(
                ExceptionCode::UNSUPPORTED_OPERATION,
                Some("VFIO-platform not supported"),
            ));
        }

        devices.iter().try_for_each(|x| bind_device(Path::new(x)))?;

        // TODO(b/278008182): create a file descriptor containing DTBO for devices.
        let (raw_read, raw_write) = pipe2(OFlag::O_CLOEXEC).map_err(|e| {
            Status::new_exception_str(
                ExceptionCode::SERVICE_SPECIFIC,
                Some(format!("can't create fd for DTBO: {e:?}")),
            )
        })?;
        // SAFETY: We are the sole owner of this FD as we just created it, and it is valid and open.
        let read_fd = unsafe { File::from_raw_fd(raw_read) };
        // SAFETY: We are the sole owner of this FD as we just created it, and it is valid and open.
        let _write_fd = unsafe { File::from_raw_fd(raw_write) };

        Ok(ParcelFileDescriptor::new(read_fd))
    }
}

lazy_static! {
    static ref DEV_VFIO: &'static Path = Path::new("/dev/vfio/vfio");
    static ref SYSFS_PLATFORM_DEVICES: &'static Path = Path::new("/sys/devices/platform/");
    static ref VFIO_PLATFORM_DRIVER: &'static Path =
        Path::new("/sys/bus/platform/drivers/vfio-platform");
    static ref SYSFS_PLATFORM_DRIVERS_PROBE: &'static Path =
        Path::new("/sys/bus/platform/drivers_probe");
    static ref IS_VFIO_SUPPORTED: bool = is_vfio_supported();
}

fn is_vfio_supported() -> bool {
    (*DEV_VFIO).exists() && (*VFIO_PLATFORM_DRIVER).exists()
}

fn check_platform_device(path: &Path) -> binder::Result<()> {
    if !path.exists() {
        return Err(Status::new_exception_str(
            ExceptionCode::ILLEGAL_ARGUMENT,
            Some(format!("no such device {path:?}")),
        ));
    }

    if !path.starts_with(*SYSFS_PLATFORM_DEVICES) {
        return Err(Status::new_exception_str(
            ExceptionCode::ILLEGAL_ARGUMENT,
            Some(format!("{path:?} is not a platform device")),
        ));
    }

    Ok(())
}

fn get_device_iommu_group(path: &Path) -> Option<u64> {
    let group_path = read_link(path.join("iommu_group")).ok()?;
    let group = group_path.file_name()?;
    group.to_str()?.parse().ok()
}

fn write_bytes_to(bytes: &[u8], path: &Path) -> binder::Result<()> {
    let mut file = OpenOptions::new().write(true).open(path).map_err(|e| {
        Status::new_exception_str(
            ExceptionCode::SERVICE_SPECIFIC,
            Some(format!("can't open {path:?}: {e:?}")),
        )
    })?;

    file.write_all(bytes).map_err(|e| {
        Status::new_exception_str(
            ExceptionCode::SERVICE_SPECIFIC,
            Some(format!("can't write to {path:?}: {e:?}")),
        )
    })
}

fn is_bound_to_vfio_driver(path: &Path) -> bool {
    let Ok(driver_path) = read_link(path.join("driver")) else {
        return false;
    };
    let Some(driver) = driver_path.file_name() else {
        return false;
    };
    driver.to_str().unwrap_or("") == "vfio-platform"
}

fn bind_vfio_driver(path: &Path) -> binder::Result<()> {
    if is_bound_to_vfio_driver(path) {
        // already bound
        return Ok(());
    }

    // unbind
    let Some(device) = path.file_name() else {
        return Err(Status::new_exception_str(
            ExceptionCode::ILLEGAL_ARGUMENT,
            Some(format!("can't get device name from {path:?}"))
        ));
    };
    let Some(device_str) = device.to_str() else {
        return Err(Status::new_exception_str(
            ExceptionCode::ILLEGAL_ARGUMENT,
            Some(format!("invalid filename {device:?}"))
        ));
    };
    write_bytes_to(device_str.as_bytes(), &path.join("driver/unbind"))?;

    // bind to VFIO
    write_bytes_to(b"vfio-platform", &path.join("driver_override"))?;
    write_bytes_to(device_str.as_bytes(), *SYSFS_PLATFORM_DRIVERS_PROBE)?;

    // final check
    if is_bound_to_vfio_driver(path) && get_device_iommu_group(path).is_some() {
        Ok(())
    } else {
        Err(Status::new_exception_str(
            ExceptionCode::SERVICE_SPECIFIC,
            Some(format!("could not bind {path:?} to VFIO platform driver")),
        ))
    }
}

fn bind_device(path: &Path) -> binder::Result<()> {
    let path = path.canonicalize().map_err(|e| {
        Status::new_exception_str(
            ExceptionCode::ILLEGAL_ARGUMENT,
            Some(format!("can't canonicalize {path:?}: {e:?}")),
        )
    })?;

    check_platform_device(&path)?;
    bind_vfio_driver(&path)
}
