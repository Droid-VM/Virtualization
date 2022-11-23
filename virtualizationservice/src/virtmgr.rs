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

//! Android Virtualization Manager

mod aidl;
mod atom;
mod composite;
mod crosvm;
mod payload;
mod selinux;

use crate::aidl::VirtualizationService;
use android_system_virtualizationservice::aidl::android::system::virtualizationservice::IVirtualizationService::BnVirtualizationService;
use anyhow::{bail, Error};
use binder::{BinderFeatures, ProcessState};
use lazy_static::lazy_static;
use log::Level;
use rpcbinder::RpcServer;
use std::os::unix::io::{FromRawFd, OwnedFd, RawFd};
use clap::Parser;
use nix::unistd::{Pid, Uid};
use std::os::unix::raw::{pid_t, uid_t};

const LOG_TAG: &str = "virtmgr";

const PID_INIT: Pid = Pid::from_raw(1);

lazy_static! {
    static ref PID_PARENT: Pid = Pid::parent();
    static ref UID_CURRENT: Uid = Uid::current();
}

fn get_calling_pid() -> pid_t {
    // The caller is the parent of this process.
    (*PID_PARENT).as_raw()
}

fn get_calling_uid() -> uid_t {
    // The caller and this process share the same UID.
    (*UID_CURRENT).as_raw()
}

#[derive(Parser)]
struct Args {
    /// Raw value of file descriptor inherited from the caller to run RpcBinder server on. This
    /// should be one end of a socketpair() compatible with RpcBinder's UDS bootstrap transport.
    #[clap(long)]
    rpc_server_fd: RawFd,
    /// Raw value of file descriptor inherited from the caller to signal RpcBinder server readiness.
    /// This should be one end of pipe() and the caller should be waiting for HUP on the other end.
    #[clap(long)]
    ready_fd: RawFd,
}

fn parse_fd_arg(raw_fd: RawFd) -> Result<OwnedFd, Error> {
    // Basic check that this value does correspond to a file descriptor.
    nix::fcntl::fcntl(raw_fd, nix::fcntl::F_GETFD)?;
    Ok(unsafe { OwnedFd::from_raw_fd(raw_fd) })
}

fn register_pdeathsig() -> Result<(), Error> {
    if *PID_PARENT == PID_INIT {
        bail!("The parent process cannot be init. The real parent had likely died");
    }

    let ret = unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) };
    if ret < 0 {
        bail!("PR_SET_PDEATHSIG failed with: {}", std::io::Error::last_os_error());
    }

    // Check for race.
    if *PID_PARENT != Pid::parent() {
        bail!("The parent process has changed during pdeathsig registration");
    }

    Ok(())
}

fn main() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag(LOG_TAG)
            .with_min_level(Level::Info)
            .with_log_id(android_logger::LogId::System),
    );

    register_pdeathsig().expect("Could not register death signal");

    let args = Args::parse();
    let ready_fd = parse_fd_arg(args.ready_fd).expect("Invalid ready fd");
    let rpc_server_fd = parse_fd_arg(args.rpc_server_fd).expect("Invalid server fd");

    // We need to start the thread pool for Binder to work properly, especially link_to_death.
    ProcessState::start_thread_pool();

    let service = VirtualizationService::init();
    let service =
        BnVirtualizationService::new_binder(service, BinderFeatures::default()).as_binder();

    let server = RpcServer::new_bootstrap_unix_domain(service, rpc_server_fd)
        .expect("Failed to start RpcServer");
    // Signal readiness to the caller by closing our end of the pipe.
    drop(ready_fd);
    server.join();
}
