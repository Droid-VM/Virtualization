// Copyright 2024, The Android Open Source Project
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

//! Library for a safer conversion from `RawFd` to `OwnedFd`

use nix::fcntl::{fcntl, FdFlag, F_SETFD};
use nix::libc;
use std::collections::HashMap;
use std::fs::read_dir;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::fd::RawFd;
use std::sync::Mutex;
use std::sync::OnceLock;
use thiserror::Error;

/// Errors that can occur while taking an ownership of `RawFd`
#[derive(Debug, Error)]
pub enum Error {
    /// init_once() not called
    #[error("init_once() not called")]
    NotInitialized,

    /// Ownership already taken
    #[error("Ownership of FD {0} is already taken")]
    OwnershipTaken(RawFd),

    /// Not an inherited file descriptor
    #[error("FD {0} is either invalid file descriptor or not an inherited one")]
    FileDescriptorNotInherited(RawFd),

    /// Failed to set CLOEXEC
    #[error("Failed to set CLOEXEC on FD {0}")]
    FailCloseOnExec(RawFd),
}

static INHERITED_FDS: OnceLock<Mutex<HashMap<RawFd, Option<OwnedFd>>>> = OnceLock::new();

/// Take ownership of all open file descriptors in this process, which later can be obtained by
/// calling `take_fd_ownership`.
///
/// # Safety
/// This function has to be called very early in the program before the ownership of any file
/// descriptors (except stdin/out/err) is taken.
pub unsafe fn init_once() -> Result<(), std::io::Error> {
    let mut fds = HashMap::new();

    for entry in read_dir("/proc/self/fd")? {
        // Files in /prod/self/fd are guaranteed to be numbers. So parsing is always successful.
        let file_name = entry?.file_name();
        let raw_fd = file_name.to_str().unwrap().parse::<RawFd>().unwrap();

        // We don't take ownership of the stdio FDs as the rust runtime owns them.
        if [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO].contains(&raw_fd) {
            continue;
        }

        // SAFETY: /proc/self/fd/* are file descriptors that are open. If `init_once()` was called
        // at the very beginning of the program execution, this is the first time to claim the
        // ownership of these file descriptors.
        let owned_fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        fds.insert(raw_fd, Some(owned_fd));
    }

    INHERITED_FDS
        .set(Mutex::new(fds))
        .or(Err(std::io::Error::other("inherited fds were already initialized")))
}

/// Take the ownership of the given `RawFd` and returns `OwnedFd` for it. The returned FD is set
/// CLOEXEC. `Error` is returned when the ownership was already taken (by a prior call to this
/// function with the same `RawFd`) or `RawFd` is not an inherited file descriptor.
pub fn take_fd_ownership(raw_fd: RawFd) -> Result<OwnedFd, Error> {
    let mut fds = INHERITED_FDS.get().ok_or(Error::NotInitialized)?.lock().unwrap();

    match fds.get(&raw_fd) {
        None => Err(Error::FileDescriptorNotInherited(raw_fd)),
        Some(None) => Err(Error::OwnershipTaken(raw_fd)),
        Some(Some(_)) => {
            // This marks that the raw_fd is taken. Some(Some(_)) deserves unwrap().unwrap()
            let owned_fd = fds.insert(raw_fd, None).unwrap().unwrap();
            fcntl(raw_fd, F_SETFD(FdFlag::FD_CLOEXEC)).or(Err(Error::FailCloseOnExec(raw_fd)))?;
            Ok(owned_fd)
        }
    }
}
