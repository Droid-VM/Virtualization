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

use nix::fcntl::{fcntl, F_DUPFD_CLOEXEC};
use nix::libc;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::fd::RawFd;
use thiserror::Error;

/// Errors that can occur while taking an ownership of `RawFd`
#[derive(Debug, PartialEq, Error)]
pub enum Error {
    /// RawFd is not a valid file descriptor
    #[error("{0} is not a file descriptor")]
    Invalid(RawFd),

    /// RawFd is either stdio, stdout, or stderr
    #[error("standard IO descriptors cannot be owned")]
    StdioNotAllowed,

    /// Generic UNIX error
    #[error("UNIX error")]
    Errno(#[from] nix::errno::Errno),
}

/// Duplicates `RawFd` and converts the dup to `OwnedFd`. It is important to know that the raw file
/// descriptor of the returned `OwnedFd` is different from `RawFd`. The returned file descriptor is
/// CLOEXEC set.
pub fn take_fd_ownership(raw_fd: RawFd) -> Result<OwnedFd, Error> {
    if [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO].contains(&raw_fd) {
        return Err(Error::StdioNotAllowed);
    }

    let new_fd = fcntl(raw_fd, F_DUPFD_CLOEXEC(raw_fd))?;

    // SAFETY: In this function, we have checked that RawFd is actually an open file descriptor and
    // this is the first time to claim its ownership because we just created it by duping.
    Ok(unsafe { OwnedFd::from_raw_fd(new_fd) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use nix::fcntl::{fcntl, FdFlag, F_GETFD, F_SETFD};
    use std::os::fd::AsRawFd;
    use std::os::fd::IntoRawFd;
    use tempfile::tempfile;

    #[test]
    fn good_fd() -> Result<()> {
        let raw_fd = tempfile()?.into_raw_fd();
        assert!(take_fd_ownership(raw_fd).is_ok());
        Ok(())
    }

    #[test]
    fn cloexec() -> Result<()> {
        let raw_fd = tempfile()?.into_raw_fd();

        // intentionally clear cloexec to see if it is set by take_fd_ownership
        fcntl(raw_fd, F_SETFD(FdFlag::empty()))?;
        let flags = fcntl(raw_fd, F_GETFD)?;
        assert_eq!(flags, FdFlag::empty().bits());

        let owned_fd = take_fd_ownership(raw_fd)?;
        let flags = fcntl(owned_fd.as_raw_fd(), F_GETFD)?;
        assert_eq!(flags, FdFlag::FD_CLOEXEC.bits());
        drop(owned_fd);
        Ok(())
    }
}
