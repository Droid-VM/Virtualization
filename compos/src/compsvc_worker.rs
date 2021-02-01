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

//! This executable works as a child/worker for the main compsvc service. This worker is mainly
//! responsible for setting up the execution environment, e.g. to create file descriptors for
//! remote file access via an authfs mount.

use anyhow::{bail, Result};
use log::warn;
use minijail::Minijail;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io;
use std::mem;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::process::exit;
use std::thread::sleep;
use std::time::{Duration, Instant};

const AUTHFS_BIN: &str = "/apex/com.android.virt/bin/authfs";
const AUTHFS_SETUP_POLL_INTERVAL_MS: Duration = Duration::from_millis(50);
const AUTHFS_SETUP_TIMEOUT_SEC: Duration = Duration::from_secs(10);
const FUSE_SUPER_MAGIC: u64 = 0x65735546;

fn is_fuse(path: &str) -> Result<bool> {
    let path = CString::new(path)?;
    // SAFETY: Zero-initialize POD struct.
    let mut st = unsafe { mem::zeroed() };
    // SAFETY: Only modify the output parameter without other side effects in this program.
    let retval = unsafe { libc::statfs(path.as_c_str().as_ptr(), &mut st) };
    if retval < 0 {
        bail!("statvfs failed: errno {}", io::Error::last_os_error());
    } else {
        Ok(st.f_type == FUSE_SUPER_MAGIC)
    }
}

fn spawn_authfs(config: &Config) -> Result<Minijail> {
    // TODO(b/185175567): Run in a more restricted sandbox.
    let jail = Minijail::new()?;

    let mut args = vec![config.authfs_root.clone()];
    for conf in &config.in_fds {
        // TODO(b/185178698): Many input files need to be signed and verified.
        // or can we use debug cert for now, which is better than nothing?iii
        args.push("--remote-ro-file-unverified".to_string());
        args.push(format!("{}:{}:{}", conf.fd, conf.fd, conf.file_size));
    }
    for conf in &config.out_fds {
        args.push("--remote-new-rw-file".to_string());
        args.push(format!("{}:{}", conf.fd, conf.fd));
    }

    let _pid = jail.run_remap(Path::new(AUTHFS_BIN), &[] /* preserve_fds */, &args)?;
    Ok(jail)
}

fn wait_until_authfs_ready(authfs_root: &str) -> Result<()> {
    let now = Instant::now();
    loop {
        if is_fuse(authfs_root)? {
            break;
        }
        if now.elapsed() > AUTHFS_SETUP_TIMEOUT_SEC {
            bail!("Time out mounting authfs");
        }
        sleep(AUTHFS_SETUP_POLL_INTERVAL_MS);
    }
    Ok(())
}

fn open_authfs_file(authfs_root: &str, basename: i32, writable: bool) -> io::Result<File> {
    OpenOptions::new().read(true).write(writable).open(format!("{}/{}", authfs_root, basename))
}

fn open_authfs_files_for_mapping(config: &Config) -> io::Result<Vec<(i32, File)>> {
    let mut fd_mapping = Vec::with_capacity(config.in_fds.len() + config.out_fds.len());

    let results: io::Result<Vec<_>> = config
        .in_fds
        .iter()
        .map(|conf| Ok((conf.fd, open_authfs_file(&config.authfs_root, conf.fd, false)?)))
        .collect();
    fd_mapping.append(&mut results?);

    let results: io::Result<Vec<_>> = config
        .out_fds
        .iter()
        .map(|conf| Ok((conf.fd, open_authfs_file(&config.authfs_root, conf.fd, true)?)))
        .collect();
    fd_mapping.append(&mut results?);

    Ok(fd_mapping)
}

fn spawn_jailed_task(args: &[String], fd_mapping: Vec<(i32, File)>) -> Result<Minijail> {
    // TODO(b/185175567): Run in a more restricted sandbox.
    let jail = Minijail::new()?;
    let preserve_fds: Vec<_> = fd_mapping.iter().map(|(id, f)| (f.as_raw_fd(), *id)).collect();
    let _pid = jail.run_remap(&Path::new(&args[0]), preserve_fds.as_slice(), &args)?;
    Ok(jail)
}

struct InFdAnnotation {
    fd: i32,
    file_size: u64,
}

struct OutFdAnnotation {
    fd: i32,
}

struct Config {
    authfs_root: String,
    in_fds: Vec<InFdAnnotation>,
    out_fds: Vec<OutFdAnnotation>,
    args: Vec<String>,
}

fn parse_args() -> Result<Config> {
    #[rustfmt::skip]
    let matches = clap::App::new("compsvc_worker")
        .arg(clap::Arg::with_name("authfs-root")
             .long("authfs-root")
             .value_name("DIR")
             .required(true)
             .takes_value(true))
        .arg(clap::Arg::with_name("in-fd")
             .long("in-fd")
             .multiple(true)
             .takes_value(true)
             .requires("authfs-root"))
        .arg(clap::Arg::with_name("out-fd")
             .long("out-fd")
             .multiple(true)
             .takes_value(true)
             .requires("authfs-root"))
        .arg(clap::Arg::with_name("args")
             .last(true)
             .required(true)
             .multiple(true))
        .get_matches();

    // Safe to unwrap since the arg is required by the clap rule
    let authfs_root = matches.value_of("authfs-root").unwrap().to_string();

    let mut in_fds = Vec::new();
    if let Some(args) = matches.values_of("in-fd") {
        for arg in args {
            if let Some(index) = arg.find(':') {
                let (fd, size) = arg.split_at(index);
                in_fds.push(InFdAnnotation { fd: fd.parse()?, file_size: size[1..].parse()? });
            } else {
                bail!("Invalid argument: {}", arg);
            };
        }
    }

    let mut out_fds = Vec::new();
    if let Some(args) = matches.values_of("out-fd") {
        for arg in args {
            out_fds.push(OutFdAnnotation { fd: arg.parse()? })
        }
    }

    let args = if let Some(args) = matches.values_of("args") {
        args.map(|s| s.to_string()).collect::<Vec<String>>()
    } else {
        unreachable!(); // the arg is required by the clap rule
    };

    Ok(Config { authfs_root, in_fds, out_fds, args })
}

fn main() -> Result<()> {
    let log_level =
        if env!("TARGET_BUILD_VARIANT") == "eng" { log::Level::Trace } else { log::Level::Info };
    android_logger::init_once(
        android_logger::Config::default().with_tag("compsvc_worker").with_min_level(log_level),
    );

    let config = parse_args()?;

    let authfs_jail = spawn_authfs(&config)?;
    let _authfs_lifetime = scopeguard::guard(authfs_jail, |authfs_jail| {
        if let Err(e) = authfs_jail.kill() {
            warn!("Failed to kill fd_server: {}", e);
        }
    });

    wait_until_authfs_ready(&config.authfs_root)?;
    let fd_mapping = open_authfs_files_for_mapping(&config)?;

    let jail = spawn_jailed_task(&config.args, fd_mapping)?;
    match jail.wait() {
        Ok(_) => Ok(()),
        Err(minijail::Error::ReturnCode(exit_code)) => {
            exit(exit_code as i32);
        }
        Err(e) => {
            bail!("Unexpected minijail error: {}", e);
        }
    }
}
