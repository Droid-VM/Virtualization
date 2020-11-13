/*
 * Copyright (C) 2020 The Android Open Source Project
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
use std::convert::TryInto;
use std::ffi::{CStr, OsStr};
use std::fs::OpenOptions;
use std::io;
use std::mem::MaybeUninit;
use std::option::Option;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::time::Duration;

use fuse;
use fuse::filesystem::{Context, DirEntry, DirectoryIterator, Entry, FileSystem, ZeroCopyWriter};
use fuse::mount::MountOption;

use crate::reader::ReadOnlyDataByChunk;

const BLOCK_SIZE: usize = 4096;

const DEFAULT_METADATA_TIMEOUT: std::time::Duration = Duration::from_secs(5);

type Inode = u64;
type Handle = u64;

pub enum FileConfig<V: ReadOnlyDataByChunk, F: ReadOnlyDataByChunk> {
    FsverityFile(V, u64), // FIXME marker?
    UnverifiedFile(F, u64),
}

struct AuthFs<V: ReadOnlyDataByChunk, F: ReadOnlyDataByChunk> {
    dir_tree: HashMap<Inode, FileConfig<V, F>>,
    max_write: u32,
}

impl<V: ReadOnlyDataByChunk, F: ReadOnlyDataByChunk> AuthFs<V, F> {
    pub fn new(dir_tree: HashMap<Inode, FileConfig<V, F>>, max_write: u32) -> AuthFs<V, F> {
        AuthFs { dir_tree, max_write }
    }

    fn get_file_config(&self, inode: &Inode) -> io::Result<&FileConfig<V, F>> {
        self.dir_tree.get(&inode).ok_or_else(|| io::Error::from_raw_os_error(libc::ENOENT))
    }
}

fn check_access_mode(flags: u32, mode: libc::c_int) -> io::Result<()> {
    if (flags & libc::O_ACCMODE as u32) == mode as u32 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(libc::EACCES))
    }
}

fn create_stat(ino: &libc::ino_t, file_size: &u64) -> io::Result<libc::stat64> {
    let mut st = unsafe { MaybeUninit::<libc::stat64>::zeroed().assume_init() };

    let file_size: i64 = (*file_size).try_into().unwrap();
    st.st_ino = *ino;
    st.st_mode = libc::S_IFREG | libc::S_IRUSR | libc::S_IRGRP | libc::S_IROTH;
    st.st_dev = 0;
    st.st_nlink = 1;
    st.st_uid = 0;
    st.st_gid = 0;
    st.st_rdev = 0;
    st.st_size = file_size;
    st.st_blksize = BLOCK_SIZE as i64;
    st.st_blocks = (file_size + 511) / 512;
    Ok(st)
}

struct EmptyDirectoryIterator {}

impl DirectoryIterator for EmptyDirectoryIterator {
    fn next(&mut self) -> Option<DirEntry> {
        None
    }
}

impl<V: ReadOnlyDataByChunk, F: ReadOnlyDataByChunk> FileSystem for AuthFs<V, F> {
    type Inode = Inode;
    type Handle = Handle;
    type DirIter = EmptyDirectoryIterator;

    fn max_buffer_size(&self) -> u32 {
        self.max_write
    }

    fn lookup(&self, _ctx: Context, _parent: Inode, name: &CStr) -> io::Result<Entry> {
        // Only accept path like /mountpoint/$num, where num will refer to the inode number.
        let num = name.to_str().map_err(|_| io::Error::from_raw_os_error(libc::ENOENT))?;
        let inode = num.parse::<Inode>().map_err(|_| io::Error::from_raw_os_error(libc::ENOENT))?;
        let st = match self.get_file_config(&inode)? {
            FileConfig::FsverityFile(_, f_size @ _) | FileConfig::UnverifiedFile(_, f_size @ _) => {
                create_stat(&inode, f_size)?
            }
        };
        Ok(Entry {
            inode,
            generation: 0,
            attr: st,
            entry_timeout: DEFAULT_METADATA_TIMEOUT,
            attr_timeout: DEFAULT_METADATA_TIMEOUT,
        })
    }

    fn getattr(
        &self,
        _ctx: Context,
        inode: Inode,
        _handle: Option<Handle>,
    ) -> io::Result<(libc::stat64, Duration)> {
        Ok((
            match self.get_file_config(&inode)? {
                FileConfig::FsverityFile(_, f_size @ _) | FileConfig::UnverifiedFile(_, f_size) => {
                    create_stat(&inode, f_size)?
                }
            },
            DEFAULT_METADATA_TIMEOUT,
        ))
    }

    fn open(
        &self,
        _ctx: Context,
        inode: Self::Inode,
        flags: u32,
    ) -> io::Result<(Option<Self::Handle>, fuse::sys::OpenOptions)> {
        match self.get_file_config(&inode)? {
            FileConfig::FsverityFile(_, _) => {
                check_access_mode(flags, libc::O_RDONLY)?;
                Ok((Some(inode), fuse::sys::OpenOptions::KEEP_CACHE))
            }
            FileConfig::UnverifiedFile(_, _) => {
                check_access_mode(flags, libc::O_RDONLY)?;
                Ok((Some(inode), fuse::sys::OpenOptions::DIRECT_IO))
            }
        }
    }

    fn read<W: io::Write + ZeroCopyWriter>(
        &self,
        _ctx: Context,
        inode: Inode,
        _handle: Handle,
        mut w: W,
        size: u32,
        offset: u64,
        _lock_owner: Option<u64>,
        _flags: u32,
    ) -> io::Result<usize> {
        match self.get_file_config(&inode)? {
            FileConfig::FsverityFile(f, f_size) => {
                if &offset == f_size {
                    return Ok(0);
                }
                let chunk_index = offset / BLOCK_SIZE as u64;
                // TODO(victorhsieh): May be able to remove this copy, if we generalize the
                // ZeroCopyReader to accept a more general trait instead of a `io::fs::File`.
                let mut buf = [0u8; BLOCK_SIZE];
                let chunk_size = f.read_chunk(chunk_index, &mut buf)?;
                let begin = (offset % BLOCK_SIZE as u64) as usize;
                let end = std::cmp::min(chunk_size, begin + size as usize);
                w.write(&buf[begin..end])
            }
            // TODO dedup
            FileConfig::UnverifiedFile(f, f_size) => {
                if &offset == f_size {
                    return Ok(0);
                }
                let chunk_index = offset / BLOCK_SIZE as u64;
                // TODO(victorhsieh): May be able to remove this copy, if we generalize the
                // ZeroCopyReader to accept a more general trait instead of a `io::fs::File`.
                let mut buf = [0u8; BLOCK_SIZE];
                let chunk_size = f.read_chunk(chunk_index, &mut buf)?;
                let begin = (offset % BLOCK_SIZE as u64) as usize;
                let end = std::cmp::min(chunk_size, begin + size as usize);
                w.write(&buf[begin..end])
            }
        }
    }
}

pub fn run<V: ReadOnlyDataByChunk + Sync, F: ReadOnlyDataByChunk + Sync>(
    dir_tree: HashMap<Inode, FileConfig<V, F>>,
) {
    // Simply set max_read to the minimum read size with fs-verity for now. We can increase the
    // read size, as long as the read implementation can read multiple chunks to fulfill the
    // request. Returning a size less than the request implies EOF has reached.
    let max_read: u32 = BLOCK_SIZE as u32;
    let max_write: u32 = 65536;
    let dev_fuse = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/fuse")
        .expect("Failed to open /dev/fuse");

    fuse::mount(
        Path::new("/tmp/mnt"),
        OsStr::new("fuse.authfs"),
        libc::MS_NOSUID | libc::MS_NODEV,
        &[
            MountOption::FD(dev_fuse.as_raw_fd()),
            MountOption::RootMode(libc::S_IFDIR | libc::S_IXUSR | libc::S_IXGRP | libc::S_IXOTH),
            MountOption::AllowOther,
            MountOption::UserId(0),
            MountOption::GroupId(0),
            MountOption::MaxRead(max_read),
        ],
    )
    .expect("Failed to mount fuse");

    // TODO deprivilege first

    if let Err(e) = fuse::worker::start_message_loop(
        dev_fuse,
        max_write,
        max_read,
        AuthFs::new(dir_tree, max_write),
    ) {
        println!("start_message_loop failed: {:?}", e);
    }
}
