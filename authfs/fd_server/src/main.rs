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

//! This program is a constrained file/FD server to serve file requests through a remote[1] binder
//! service. The file server is not designed to serve arbitrary file paths in the filesystem. On
//! the contrary, the server should be configured to starts with already opened FDs, and serve the
//! client's request against the FDs
//!
//! For example, `exec 9</path/to/file fd_server --ro-fds 9` starts the binder service. A client
//! client can then request the content of file 9 by offset and size.
//!
//! [1] Since the remote binder is not ready, this currently implementation uses local binder
//!     first.

use std::collections::BTreeMap;
use std::convert::TryInto;
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::os::unix::io::FromRawFd;

use anyhow::{anyhow, bail, Result};
use binder::{public_api::Result as BinderResult, ExceptionCode, IBinder, Interface, Status};
use log::{debug, error};

use authfs_aidl_interface::aidl::com::android::virt::fs::IVirtFdService::{
    BnVirtFdService, IVirtFdService,
};

static SERVICE_NAME: &str = "authfs_fd_server";

/// Maximum buffer size of a request, just to avoid allocate and transfer a huge buffer.
const MAX_REQUESTING_DATA: i32 = 16384;

/// Error code used as the binder service specific error.
enum ErrorCode {
    Io = 1,
    FileNotFound = 2,
}

impl From<ErrorCode> for Status {
    fn from(error: ErrorCode) -> Self {
        Status::new_service_specific_error(error as i32, None)
    }
}

fn new_binder_exception(exception: ExceptionCode, message: &str) -> Status {
    Status::new_exception(exception, CString::new(message).as_deref().ok())
}

/// Configuration of a read-only file to serve by this server. The file is supposed to be verifiable
/// with the associcated fs-verity metadata.
struct ReadonlyFdConfig {
    /// The file to read from. fs-verity metadata can be retrieved from this file's FD.
    file: File,

    /// Alternative Merkle tree stored in another file.
    alt_merkle_file: Option<File>,

    /// Alternative signature stored in another file.
    alt_signature_file: Option<File>,
}

struct FdService {
    /// A pool of read-only files
    fd_pool: BTreeMap<i32, ReadonlyFdConfig>,
}

impl FdService {
    pub fn new_binder(fd_pool: BTreeMap<i32, ReadonlyFdConfig>) -> Result<impl IVirtFdService> {
        let result = BnVirtFdService::new_binder(FdService { fd_pool });
        result.as_binder().set_requesting_sid(false);
        Ok(result)
    }

    fn get_file_config(&self, id: i32) -> BinderResult<&ReadonlyFdConfig> {
        self.fd_pool.get(&id).ok_or_else(|| Status::from(ErrorCode::FileNotFound))
    }

    fn get_file(&self, id: i32) -> BinderResult<&File> {
        Ok(&self.get_file_config(id)?.file)
    }
}

impl Interface for FdService {}

impl IVirtFdService for FdService {
    fn readFile(&self, id: i32, offset: i64, size: i32) -> BinderResult<Vec<u8>> {
        if size > MAX_REQUESTING_DATA {
            return Err(new_binder_exception(
                ExceptionCode::ILLEGAL_ARGUMENT,
                "Unexpect large size",
            ));
        }
        let size: usize = size
            .try_into()
            .map_err(|_| new_binder_exception(ExceptionCode::ILLEGAL_ARGUMENT, "Invalid size"))?;
        let offset: u64 = offset
            .try_into()
            .map_err(|_| new_binder_exception(ExceptionCode::ILLEGAL_ARGUMENT, "Invalid offset"))?;

        read_exact_into_buf(self.get_file(id)?, size, offset).map_err(|e| {
            error!("readFile: read error: {}", e);
            Status::from(ErrorCode::Io)
        })
    }

    fn readFsverityMerkleTree(&self, id: i32, offset: i64, size: i32) -> BinderResult<Vec<u8>> {
        if size > MAX_REQUESTING_DATA {
            return Err(new_binder_exception(
                ExceptionCode::ILLEGAL_ARGUMENT,
                "Unexpect large size",
            ));
        }
        let size: usize = size
            .try_into()
            .map_err(|_| new_binder_exception(ExceptionCode::ILLEGAL_ARGUMENT, "Invalid size"))?;
        let offset: u64 = offset
            .try_into()
            .map_err(|_| new_binder_exception(ExceptionCode::ILLEGAL_ARGUMENT, "Invalid offset"))?;

        if let Some(file) = &self.get_file_config(id)?.alt_merkle_file {
            read_exact_into_buf(&file, size, offset).map_err(|e| {
                error!("readFsverityMerkleTree: read error: {}", e);
                Status::from(ErrorCode::Io)
            })
        } else {
            // TODO(victorhsieh) retrieve from the fd when the new ioctl is ready
            Err(new_binder_exception(ExceptionCode::UNSUPPORTED_OPERATION, "Not implemented yet"))
        }
    }

    fn readFsveritySignature(&self, id: i32) -> BinderResult<Vec<u8>> {
        if let Some(file) = &self.get_file_config(id)?.alt_signature_file {
            // Supposedly big enough buffer size to store signature.
            let size = MAX_REQUESTING_DATA as usize;
            read_exact_into_buf(&file, size, 0).map_err(|e| {
                error!("readFsveritySignature: read error: {}", e);
                Status::from(ErrorCode::Io)
            })
        } else {
            // TODO(victorhsieh) retrieve from the fd when the new ioctl is ready
            Err(new_binder_exception(ExceptionCode::UNSUPPORTED_OPERATION, "Not implemented yet"))
        }
    }
}

fn read_exact_into_buf(file: &File, size: usize, offset: u64) -> io::Result<Vec<u8>> {
    let remaining = file.metadata()?.len() - offset;
    let buf_size = std::cmp::min(remaining, size as u64) as usize;
    debug_assert!(buf_size <= MAX_REQUESTING_DATA as usize);
    let mut buf = vec![0; buf_size];

    // This loop simulates `read_exact`, but with `read_at` to keep the file immutable.
    let mut progress = 0;
    while progress < buf.len() {
        match file.read_at(&mut buf[progress..], offset + progress as u64) {
            Ok(0) => break,
            Ok(n) => progress += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
        }
    }
    debug_assert!(progress == buf.len());
    Ok(buf)
}

fn is_fd_valid(fd: i32) -> bool {
    // SAFETY: a query-only syscall
    let retval = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    retval >= 0
}

fn fd_to_file(fd: i32) -> Result<File> {
    if !is_fd_valid(fd) {
        bail!("Bad FD: {}", fd);
    }
    // SAFETY: The caller is supposed to provide valid FDs to this process.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn parse_arg_ro_fds(arg: &str) -> Result<(i32, ReadonlyFdConfig)> {
    let result: Result<Vec<i32>, _> = arg.split(':').map(|x| x.parse::<i32>()).collect();
    let fds = result?;
    if fds.len() > 3 {
        bail!("Too many options: {}", arg);
    }

    Ok((
        fds[0],
        ReadonlyFdConfig {
            file: fd_to_file(fds[0])?,
            alt_merkle_file: fds.get(1).map(|fd| fd_to_file(*fd)).transpose()?,
            alt_signature_file: fds.get(2).map(|fd| fd_to_file(*fd)).transpose()?,
        },
    ))
}

fn parse_args() -> Result<BTreeMap<i32, ReadonlyFdConfig>> {
    #[rustfmt::skip]
    let matches = clap::App::new("fd_server")
        .arg(clap::Arg::with_name("ro-fds")
             .long("ro-fds")
             .required(true)
             .multiple(true)
             .number_of_values(1))
        .get_matches();

    let mut fd_pool = BTreeMap::new();
    if let Some(args) = matches.values_of("ro-fds") {
        for arg in args {
            let (fd, config) = parse_arg_ro_fds(arg)?;
            fd_pool.insert(fd, config);
        }
    }
    Ok(fd_pool)
}

fn main() -> Result<()> {
    let fd_pool = parse_args()?;

    binder::ProcessState::start_thread_pool();

    let service = FdService::new_binder(fd_pool).unwrap_or_else(|e| {
        panic!("Failed to create service {}: {}", SERVICE_NAME, e);
    });

    binder::add_service(SERVICE_NAME, service.as_binder()).unwrap_or_else(|e| {
        panic!("Failed to register service {}: {}", SERVICE_NAME, e);
    });

    debug!("fd_server is running.");

    binder::ProcessState::join_thread_pool();
    Err(anyhow!("Unexpected exit after join_thread_pool"))
}
