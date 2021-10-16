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

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use super::remote_file::RemoteFileEditor;
use super::VirtFdService;
use crate::fsverity::VerifiedFileEditor;
use crate::fusefs::Inode;

/// A remote directory backed by a remote directory FD, where the provider/fd_server is not
/// trusted. The directory is assumed empty initially.
///
/// A process can create new files and directories (with same assumption described above) in the
/// directory. Integrity of new files are maintained within the VM. Similarly, the list of directory
/// entries are kept within authfs such that it won't be affected by compromised fd_server who may
/// provide wrong data.
pub struct RemoteDirEditor {
    service: VirtFdService,
    remote_dir_fd: i32,

    /// Mapping of entry names to the corresponding inode number. The actual file/directory is
    /// stored in the global pool in fusefs.
    entries: HashMap<PathBuf, Inode>,
}

impl RemoteDirEditor {
    pub fn new(service: VirtFdService, remote_dir_fd: i32) -> Self {
        RemoteDirEditor { service, remote_dir_fd, entries: HashMap::new() }
    }

    /// Returns the number of entries created.
    pub fn number_of_entries(&self) -> usize {
        self.entries.len()
    }

    /// Creates a remote file at the current directory. If succeed, the returned remote FD is
    /// stored in `entries` as the inode number.
    pub fn create_file(
        &mut self,
        basename: &Path,
    ) -> io::Result<(Inode, VerifiedFileEditor<RemoteFileEditor>)> {
        if self.entries.contains_key(basename) {
            return Err(io::Error::from_raw_os_error(libc::EEXIST));
        }

        let basename_str =
            basename.to_str().ok_or_else(|| io::Error::from_raw_os_error(libc::EINVAL))?;
        let new_fd =
            self.service.createFileInDirectory(self.remote_dir_fd, basename_str).map_err(|e| {
                let maybe_errno = e.service_specific_error();
                if maybe_errno > 0 {
                    io::Error::from_raw_os_error(maybe_errno) // errno
                } else {
                    io::Error::new(io::ErrorKind::Other, e.get_description())
                }
            })?;
        let new_inode = new_fd as Inode;

        let new_remote_file =
            VerifiedFileEditor::new(RemoteFileEditor::new(self.service.clone(), new_fd));
        self.entries.insert(basename.to_path_buf(), new_inode);
        Ok((new_inode, new_remote_file))
    }

    /// Creates a remote directory at the current directory. If succeed, the returned remote FD is
    /// stored in `entries` as the inode number.
    pub fn mkdir(&mut self, basename: &Path) -> io::Result<(Inode, RemoteDirEditor)> {
        if self.entries.contains_key(basename) {
            return Err(io::Error::from_raw_os_error(libc::EEXIST));
        }
        let basename_str =
            basename.to_str().ok_or_else(|| io::Error::from_raw_os_error(libc::EINVAL))?;
        let new_fd = self.service.mkdir(self.remote_dir_fd, basename_str).map_err(|e| {
            let maybe_errno = e.service_specific_error();
            if maybe_errno > 0 {
                io::Error::from_raw_os_error(maybe_errno) // errno
            } else {
                io::Error::new(io::ErrorKind::Other, e.get_description())
            }
        })?;
        let new_inode = new_fd as Inode;

        let new_remote_dir = RemoteDirEditor::new(self.service.clone(), new_fd);
        self.entries.insert(basename.to_path_buf(), new_inode);
        Ok((new_inode, new_remote_dir))
    }

    /// Returns the inode number of a file or directory named `name` previously created through
    /// `RemoteDirEditor`.
    pub fn find_inode(&self, name: &Path) -> Option<Inode> {
        self.entries.get(name).copied()
    }
}
