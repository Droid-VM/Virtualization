// Copyright 2022, The Android Open Source Project
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

//! Main entry point for the microdroid IDiceDevice HAL implementation.

use anyhow::{bail, Result};
use byteorder::{NativeEndian, ReadBytesExt};
use diced::{
    dice,
    hal_node::{DiceArtifacts, DiceDevice, ResidentHal, UpdatableDiceArtifacts},
};
use libc::{c_void, mmap, MAP_FAILED, MAP_PRIVATE, PROT_READ};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::panic;
use std::ptr::null_mut;
use std::sync::Arc;

static DICE_DEV_PATH: &str = "/dev/dice0";
static DICE_HAL_SERVICE_NAME: &str = "android.hardware.security.dice.IDiceDevice/default";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct DriverMmapArtifacts {
    mmap: *mut c_void,
    cdi_attest: *const [u8; dice::CDI_SIZE],
    cdi_seal: *const [u8; dice::CDI_SIZE],
    bcc: *const [u8],
    bcc_size: usize,
}

// TODO: impl Drop to unmap

impl DiceArtifacts for DriverMmapArtifacts {
    fn cdi_attest(&self) -> &[u8; dice::CDI_SIZE] {
        self.cdi_attest.as_ref().unwrap()
    }
    fn cdi_seal(&self) -> &[u8; dice::CDI_SIZE] {
        self.cdi_seal.as_ref().unwrap()
    }
    fn bcc(&self) -> Vec<u8> {
        slice::from_raw_parts(self.bcc.as_ref().unwrap(), self.bcc_size).to_vec()
    }
}

struct DriverArtifacts {}

impl UpdatableDiceArtifacts for DriverArtifacts {
    fn with_artifacts<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&dyn DiceArtifacts) -> Result<T>,
    {
        // Based on example code for driver interface at
        // https://android-review.googlesource.com/c/kernel/common/+/1943619/3/drivers/misc/open-dice.c#15
        let file = File::open(DICE_DEV_PATH)?;
        let size = file.read_u64::<NativeEndian>()? as usize;
        let data = unsafe {
            let fd = file.as_raw_fd();
            let data = mmap(null_mut(), size, PROT_READ, MAP_PRIVATE, fd, 0);
            if data == MAP_FAILED {
                bail!("Failed to mmap {}", DICE_DEV_PATH);
            }
            // TODO: parse BccHandover to get pointer to fields
            data
        };
        f(&DriverMmapArtifacts {
            mmap: data,
            cdi_attest: null_mut(),
            cdi_seal: null_mut(),
            bcc: null_mut(),
            bcc_size: 0,
        })
    }
    fn update(self, new_artifacts: &impl DiceArtifacts) -> Result<Self> {
        // TODO: do something like putting the new values in memory somewhere
        Ok(Self)
    }
}

fn main() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("android.hardware.security.dice")
            .with_min_level(log::Level::Debug),
    );
    // Redirect panic messages to logcat.
    panic::set_hook(Box::new(|panic_info| {
        log::error!("{}", panic_info);
    }));

    // Saying hi.
    log::info!("android.hardware.security.dice is starting.");

    let hal_impl = Arc::new(
        unsafe {
            // Safety: ResidentHal cannot be used in multi threaded processes.
            // This service does not start a thread pool. The main thread is the only thread
            // joining the thread pool, thereby keeping the process single threaded.
            ResidentHal::new(DriverArtifacts {})
        }
        .expect("Failed to create ResidentHal implementation."),
    );

    let hal = DiceDevice::new_as_binder(hal_impl).expect("Failed to construct hal service.");

    binder::add_service(DICE_HAL_SERVICE_NAME, hal.as_binder())
        .expect("Failed to register IDiceDevice Service");

    log::info!("Joining thread pool now.");
    binder::ProcessState::join_thread_pool();
}
