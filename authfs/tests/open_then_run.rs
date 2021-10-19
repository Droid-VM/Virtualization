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

//! This is a test helper program that opens files and/or directories, then pass the file
//! descriptor to the specified command. When passing the file descriptors, they are mapped to the
//! specified numbers in the child process.

use anyhow::{bail, Context, Result};
use clap::{App, Arg};
use command_fds::{CommandFdExt, FdMapping};
use log::{debug, error};
use nix::{dir::Dir, fcntl::OFlag, sys::stat::Mode};
use std::fs::{File, OpenOptions};
use std::os::unix::io::{AsRawFd, RawFd};
use std::process::Command;

type PseudoRawFd = RawFd;

fn parse_option(option: &str) -> Result<(PseudoRawFd, &str)> {
    // Example option: 10:/some/path
    let strs: Vec<&str> = option.split(':').collect();
    if strs.len() != 2 {
        bail!("Invalid option: {}", option);
    }
    Ok((strs[0].parse::<PseudoRawFd>().context("Invalid FD format")?, strs[1]))
}

struct Args {
    ro_files: Vec<(PseudoRawFd, File)>,
    rw_files: Vec<(PseudoRawFd, File)>,
    dir_files: Vec<(PseudoRawFd, Dir)>,
    cmdline_args: Vec<String>,
}

fn parse_args() -> Result<Args> {
    #[rustfmt::skip]
    let matches = App::new("open_then_run")
        .arg(Arg::with_name("open-ro")
             .long("open-ro")
             .value_name("FD:PATH")
             .help("Open <PATH> read-only to pass as fd <FD>")
             .multiple(true)
             .number_of_values(1))
        .arg(Arg::with_name("open-rw")
             .long("open-rw")
             .value_name("FD:PATH")
             .help("Open/create <PATH> read-write to pass as fd <FD>")
             .multiple(true)
             .number_of_values(1))
        .arg(Arg::with_name("open-dir")
             .long("open-dir")
             .value_name("FD:DIR")
             .help("Open <DIR> to pass as fd <FD>")
             .multiple(true)
             .number_of_values(1))
        .arg(Arg::with_name("args")
             .help("Command line to execute with pre-opened FD inherited")
             .last(true)
             .required(true)
             .multiple(true))
        .get_matches();

    let results: Result<Vec<_>> = if let Some(options) = matches.values_of("open-ro") {
        options
            .map(|option| {
                let (fd, path) = parse_option(option)?;
                Ok((
                    fd,
                    OpenOptions::new()
                        .read(true)
                        .open(path)
                        .with_context(|| format!("Open {} read-only as FD {}", path, fd))?,
                ))
            })
            .collect()
    } else {
        Ok(Vec::new())
    };
    let ro_files = results?;

    let results: Result<Vec<_>> = if let Some(options) = matches.values_of("open-rw") {
        options
            .map(|option| {
                let (fd, path) = parse_option(option)?;
                Ok((
                    fd,
                    OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .open(path)
                        .with_context(|| format!("Open {} read-write as FD {}", path, fd))?,
                ))
            })
            .collect()
    } else {
        Ok(Vec::new())
    };
    let rw_files = results?;

    let results: Result<Vec<_>> = if let Some(options) = matches.values_of("open-dir") {
        options
            .map(|option| {
                let (fd, path) = parse_option(option)?;
                Ok((
                    fd,
                    Dir::open(path, OFlag::O_DIRECTORY | OFlag::O_RDWR, Mode::S_IRWXU)
                        .with_context(|| format!("Open {} directory as FD {}", path, fd))?,
                ))
            })
            .collect()
    } else {
        Ok(Vec::new())
    };
    let dir_files = results?;

    let cmdline_args: Vec<_> = matches.values_of("args").unwrap().map(|s| s.to_string()).collect();

    Ok(Args { ro_files, rw_files, dir_files, cmdline_args })
}

fn as_fd_mapping<T: AsRawFd>(option: &(PseudoRawFd, T)) -> FdMapping {
    FdMapping { parent_fd: option.1.as_raw_fd(), child_fd: option.0 }
}

fn try_main() -> Result<()> {
    let args = parse_args()?;

    let mut command = Command::new(&args.cmdline_args[0]);
    command.args(&args.cmdline_args[1..]);

    // Set up FD mappings in the child process.
    let mut fd_mappings = Vec::new();
    fd_mappings.extend(args.ro_files.iter().map(as_fd_mapping));
    fd_mappings.extend(args.rw_files.iter().map(as_fd_mapping));
    fd_mappings.extend(args.dir_files.iter().map(as_fd_mapping));
    command.fd_mappings(fd_mappings)?;

    debug!("Spawning {:?}", command);
    command.spawn()?;
    Ok(())
}

fn main() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("open_and_run")
            .with_min_level(log::Level::Debug),
    );

    if let Err(e) = try_main() {
        error!("Failed with {:?}", e);
        std::process::exit(1);
    }
}
