// Copyright 2021, The Android Open Source Project
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

//! Android VirtualizationService

// TODO(b/245727626) - remove after split from virtmgr
#![allow(dead_code)]

mod aidl;
mod atom;
mod composite;
mod crosvm;
mod payload;
mod selinux;

use crate::aidl::{VirtualizationServiceInternal, BINDER_SERVICE_IDENTIFIER, TEMPORARY_DIRECTORY};
use android_logger::{Config, FilterBuilder};
use android_system_virtualizationservice::aidl::android::system::virtualizationservice::IVirtualizationServiceInternal::BnVirtualizationServiceInternal;
use anyhow::Error;
use binder::{register_lazy_service, BinderFeatures, ProcessState, ThreadState};
use log::{info, Level};
use std::fs::{remove_dir_all, remove_file, read_dir};
use std::os::unix::raw::{pid_t, uid_t};

const LOG_TAG: &str = "VirtualizationService";

fn get_calling_pid() -> pid_t {
    ThreadState::get_calling_pid()
}

fn get_calling_uid() -> uid_t {
    ThreadState::get_calling_uid()
}

fn main() {
    android_logger::init_once(
        Config::default()
            .with_tag(LOG_TAG)
            .with_min_level(Level::Info)
            .with_log_id(android_logger::LogId::System)
            .with_filter(
                // Reduce logspam by silencing logs from the disk crate which don't provide much
                // information to us.
                FilterBuilder::new().parse("info,disk=off").build(),
            ),
    );

    remove_memlock_rlimit(RLIMIT_MEMLOCK_MAX).expect("Failed to remove memlock rlimit");
    clear_temporary_files().expect("Failed to delete old temporary files");

    let service = VirtualizationServiceInternal::init();
    let service = BnVirtualizationServiceInternal::new_binder(service, BinderFeatures::default());
    register_lazy_service(BINDER_SERVICE_IDENTIFIER, service.as_binder()).unwrap();
    info!("Registered Binder service, joining threadpool.");
    ProcessState::join_thread_pool();
}

/// Set this PID's rlimit for RLIMIT_MEMLOCK to RLIM_INFINITY.
fn remove_memlock_rlimit() -> Result<(), Error> {
    let lim = libc::rlimit { rlim_cur: libc::RLIM_INFINITY, rlim_max: libc::RLIM_INFINITY };
    // SAFETY - borrowing the new limit struct only
    match unsafe { libc::setrlimit(libc::RLIMIT_MEMLOCK, &lim) } {
        0 => Ok(()),
        -1 => Err(std::io::Error::last_os_error()),
        n => bail!("Unexpected return value from setrlimit(): {}", n),
    }
}

/// Remove any files under `TEMPORARY_DIRECTORY`.
fn clear_temporary_files() -> Result<(), Error> {
    for dir_entry in read_dir(TEMPORARY_DIRECTORY)? {
        let dir_entry = dir_entry?;
        let path = dir_entry.path();
        if dir_entry.file_type()?.is_dir() {
            remove_dir_all(path)?;
        } else {
            remove_file(path)?;
        }
    }
    Ok(())
}
