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

//! pvm_exec is a proxy/wrapper command to run a command remotely. It does not transport the
//! program and just pass the command line arguments to compsvc to execute. The most imporant task
//! for this program is to run a `fd_server` that serves remote file read/write requests.
//!
//! Example:
//! $ adb shell exec 3</dev/zero 4<>/dev/null pvm_exec --in-fd 3 --out-fd 4 -- sleep 10
//!
//! Note the immediate argument right after "--" (e.g. "sleep" in the example above) is not really
//! used. It is only for ergonomics.

use anyhow::{bail, Context, Result};
use libc::{fcntl, fstat, stat, F_GETFD};
use log::{error, warn};
use minijail::Minijail;
use std::mem::MaybeUninit;
use std::path::Path;
use std::process::exit;

use compos_aidl_interface::aidl::com::android::virt::compos::{
    ICompService::ICompService, InputFdAnnotation::InputFdAnnotation, Metadata::Metadata,
    OutputFdAnnotation::OutputFdAnnotation,
};
use compos_aidl_interface::binder::Strong;

static SERVICE_NAME: &str = "compsvc";
static FD_SERVER_BIN: &str = "/apex/com.android.virt/bin/fd_server";

fn get_local_service() -> Strong<dyn ICompService> {
    compos_aidl_interface::binder::get_interface(SERVICE_NAME).expect("Cannot reach compsvc")
}

fn spawn_fd_server(metadata: &Metadata) -> Result<Minijail> {
    let mut args = Vec::new();
    for metadata in &metadata.input_fd_annotations {
        args.push("--ro-fds".to_string());
        args.push(metadata.fd.to_string());
    }
    for metadata in &metadata.output_fd_annotations {
        args.push("--rw-fds".to_string());
        args.push(metadata.fd.to_string());
    }

    let inheritable_fds = Vec::new();
    let jail = Minijail::new()?;
    let _pid = jail.run(Path::new(FD_SERVER_BIN), &inheritable_fds, &args)?;
    Ok(jail)
}

fn is_fd_valid(fd: i32) -> bool {
    // SAFETY: a read-only syscall that does not have a state changing side effect.
    let retval = unsafe { fcntl(fd, F_GETFD) };
    retval >= 0
}

fn parse_arg_fd(arg: &str) -> Result<i32> {
    let fd = arg.parse::<i32>()?;
    if !is_fd_valid(fd) {
        bail!("Bad FD: {}", fd);
    }
    Ok(fd)
}

fn size_from_fd(fd: i32) -> Result<i64> {
    let mut st = MaybeUninit::<stat>::uninit();
    // SAFETY: a read-only syscall that does not have a state changing side effect.
    let retval = unsafe { fstat(fd, st.as_mut_ptr()) };
    if retval < 0 {
        bail!("fstat failed: {}", retval);
    } else {
        // SAFETY: fstat(2) should have initialized the struct on success.
        Ok(unsafe { st.assume_init().st_size })
    }
}

fn parse_args() -> Result<(Vec<String>, Metadata)> {
    #[rustfmt::skip]
    let matches = clap::App::new("pvm_exec")
        .arg(clap::Arg::with_name("in-fd")
             .long("in-fd")
             .takes_value(true)
             .multiple(true)
             .use_delimiter(true))
        .arg(clap::Arg::with_name("out-fd")
             .long("out-fd")
             .takes_value(true)
             .multiple(true)
             .use_delimiter(true))
        .arg(clap::Arg::with_name("args")
             .last(true)
             .required(true)
             .multiple(true))
        .get_matches();

    let input_fd_annotations = if let Some(args) = matches.values_of("in-fd") {
        let results: Result<Vec<_>> = args
            .map(|arg| {
                let fd = parse_arg_fd(arg)?;
                let file_size = size_from_fd(fd)?;
                Ok(InputFdAnnotation { fd, file_size })
            })
            .collect();
        results?
    } else {
        Vec::new()
    };

    let output_fd_annotations = if let Some(args) = matches.values_of("out-fd") {
        let results: Result<Vec<_>> = args
            .map(|arg| {
                let fd = parse_arg_fd(arg)?;
                Ok(OutputFdAnnotation { fd })
            })
            .collect();
        results?
    } else {
        Vec::new()
    };

    let args = if let Some(args) = matches.values_of("args") {
        args.map(|s| s.to_string()).collect::<Vec<String>>()
    } else {
        unreachable!(); // the arg is required
    };

    Ok((args, Metadata { input_fd_annotations, output_fd_annotations }))
}

fn main() -> Result<()> {
    // 1. Parse the command line arguments for collect execution data.
    let (args, metadata) = parse_args()?;

    // 2. Spawn and configure a fd_server to serve remote read/write requests.
    let fd_server_jail = spawn_fd_server(&metadata)?;
    let _fd_server_lifetime = scopeguard::guard(fd_server_jail, |fd_server_jail| {
        if let Err(e) = fd_server_jail.kill() {
            warn!("Failed to kill fd_server: {}", e);
        }
    });

    // 3. Send the command line args to the remote to execute.
    let exit_code = get_local_service().execute(&args, &metadata).context("Binder call failed")?;

    if exit_code > 0 {
        error!("remote execution failed with exit code {}", exit_code);
        exit(exit_code as i32);
    }
    Ok(())
}
