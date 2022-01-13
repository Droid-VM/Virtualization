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
use libc::{c_void, mmap, munmap, MAP_FAILED, MAP_PRIVATE, PROT_READ};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::io::AsRawFd;
use std::panic;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::slice;
use std::sync::Arc;

static DICE_HAL_SERVICE_NAME: &str = "android.hardware.security.dice.IDiceDevice/default";

struct MappedDriverArtifacts<'a> {
    mmap_addr: *mut c_void,
    mmap_size: usize,
    cdi_attest: &'a [u8; dice::CDI_SIZE],
    cdi_seal: &'a [u8; dice::CDI_SIZE],
    bcc: &'a [u8],
}

impl MappedDriverArtifacts<'_> {
    fn new(driver_path: &Path) -> Result<Self> {
        let mut file = fs::File::open(driver_path)?;
        let mmap_size = file.read_u64::<NativeEndian>()? as usize;
        let mmap_addr = unsafe {
            let fd = file.as_raw_fd();
            mmap(null_mut(), mmap_size, PROT_READ, MAP_PRIVATE, fd, 0)
        };
        if mmap_addr == MAP_FAILED {
            bail!("Failed to mmap {}", driver_path.display());
        }
        let mmap_buf =
            unsafe { slice::from_raw_parts((mmap_addr as *const u8).as_ref().unwrap(), mmap_size) };
        // Very inflexible parsing / validation of the BccHandover data.
        if mmap_buf[0..4] != [0xa3, 0x01, 0x58, 0x20]
            || mmap_buf[36..39] != [0x02, 0x58, 0x20]
            || mmap_buf[71] != 0x03
        {
            bail!("BccHandover format mismatch");
        }
        Ok(Self {
            mmap_addr,
            mmap_size,
            cdi_attest: mmap_buf[4..36].try_into().unwrap(),
            cdi_seal: mmap_buf[39..71].try_into().unwrap(),
            bcc: &mmap_buf[72..],
        })
    }
}

impl Drop for MappedDriverArtifacts<'_> {
    fn drop(&mut self) {
        let ret = unsafe { munmap(self.mmap_addr, self.mmap_size) };
        if ret != 0 {
            log::warn!("Failed to munmap ({})", ret);
        }
    }
}

impl DiceArtifacts for MappedDriverArtifacts<'_> {
    fn cdi_attest(&self) -> &[u8; dice::CDI_SIZE] {
        self.cdi_attest
    }
    fn cdi_seal(&self) -> &[u8; dice::CDI_SIZE] {
        self.cdi_seal
    }
    fn bcc(&self) -> Vec<u8> {
        self.bcc.to_vec()
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct RawArtifacts {
    cdi_attest: [u8; dice::CDI_SIZE],
    cdi_seal: [u8; dice::CDI_SIZE],
    bcc: Vec<u8>,
}

impl DiceArtifacts for RawArtifacts {
    fn cdi_attest(&self) -> &[u8; dice::CDI_SIZE] {
        &self.cdi_attest
    }
    fn cdi_seal(&self) -> &[u8; dice::CDI_SIZE] {
        &self.cdi_seal
    }
    fn bcc(&self) -> Vec<u8> {
        self.bcc.clone()
    }
}

#[derive(Clone, Serialize, Deserialize)]
enum DriverArtifactManager {
    Driver(PathBuf),
    Updated(RawArtifacts),
}

impl DriverArtifactManager {
    fn new(driver_path: &Path) -> Self {
        Self::Driver(driver_path.to_path_buf())
    }
}

impl UpdatableDiceArtifacts for DriverArtifactManager {
    fn with_artifacts<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&dyn DiceArtifacts) -> Result<T>,
    {
        match self {
            Self::Driver(driver_path) => f(&MappedDriverArtifacts::new(driver_path.as_path())?),
            Self::Updated(raw_artifacts) => f(raw_artifacts),
        }
    }
    fn update(self, new_artifacts: &impl DiceArtifacts) -> Result<Self> {
        if let Self::Driver(driver_path) = self {
            fs::write(driver_path, "wipe")?;
        }
        Ok(Self::Updated(RawArtifacts {
            cdi_attest: *new_artifacts.cdi_attest(),
            cdi_seal: *new_artifacts.cdi_seal(),
            bcc: new_artifacts.bcc(),
        }))
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
            ResidentHal::new(DriverArtifactManager::new(Path::new("/dev/dice0")))
        }
        .expect("Failed to create ResidentHal implementation."),
    );

    let hal = DiceDevice::new_as_binder(hal_impl).expect("Failed to construct hal service.");

    binder::add_service(DICE_HAL_SERVICE_NAME, hal.as_binder())
        .expect("Failed to register IDiceDevice Service");

    log::info!("Joining thread pool now.");
    binder::ProcessState::join_thread_pool();
}
