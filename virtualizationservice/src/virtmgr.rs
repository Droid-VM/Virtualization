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

// TODO(b/245727626) - remove after split from virtualizationservice
#![allow(dead_code)]

mod aidl;
mod atom;
mod composite;
mod crosvm;
mod payload;
mod selinux;

use crate::aidl::VirtualizationService;
use android_system_virtualizationservice::aidl::android::system::virtualizationservice::IVirtualizationService::BnVirtualizationService;
use binder::BinderFeatures;
use log::{error, Level};
use rpcbinder::run_unix_bootstrap_rpc_server;
use std::os::unix::io::{FromRawFd, OwnedFd, RawFd};
use clap::Parser;
use nix::unistd::Pid;

const LOG_TAG: &str = "virtmgr";

const PID_INIT: Pid = Pid::from_raw(1);

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

fn parse_fd_arg(raw_fd: RawFd) -> Result<OwnedFd, nix::Error> {
    // Basic check that this value does correspond to a file descriptor.
    nix::fcntl::fcntl(raw_fd, nix::fcntl::F_GETFD)?;
    Ok(unsafe { OwnedFd::from_raw_fd(raw_fd) })
}

fn register_pdeathsig() -> Result<(), std::io::Error> {
    let ppid = Pid::parent();
    if ppid == PID_INIT {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "parent is init"));
    }

    // TODO: Use SIGTERM to gracefully terminate the process. That requires
    // manually terminating crosvm instances as well. SIGKILL kills children
    // automatically.
    let ret = unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) };
    if ret < 0 {
        return Err(std::io::Error::last_os_error());
    }

    // Check for race.
    if ppid != Pid::parent() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "parent changed"));
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

    let service = VirtualizationService::init();
    let service =
        BnVirtualizationService::new_binder(service, BinderFeatures::default()).as_binder();

    let ret = run_unix_bootstrap_rpc_server(service, rpc_server_fd, || {
        // Signal readiness to the caller by closing our end of the pipe.
        drop(ready_fd);
    });
    if !ret {
        error!("Premature termination of RPC server");
    }
}
