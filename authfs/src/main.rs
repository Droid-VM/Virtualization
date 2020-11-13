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

//! This crate implements AuthFS, a FUSE-based, non-generic filesystem where file access is
//! authenticated. This filesystem assumes the underlying layer is not trusted, e.g. file may be
//! provided by an untrusted host/VM, so that the content can't be simply trusted. However, with a
//! public key from a trusted party, this filesystem can still verify a (read-only) file signed by
//! the trusted party even if the host/VM as the blob provider is malicious. With the Merkle tree,
//! each read of file block can be verified individually only when needed.
//!
//! AuthFS only serve files that are specifically configured. A file configuration may include the
//! source (e.g. local file or remote file server), verification method (e.g. certificate for
//! fs-verity verification, or no verification if expected to mount over dm-verity), and file ID.
//! Regardless of the actual file name, the exposed file names through AuthFS are currently integer,
//! e.g. /mountpoint/42.

use anyhow::{bail, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use structopt::StructOpt;

mod auth;
mod crypto;
mod fsverity;
mod fusefs;
mod reader;

use auth::FakeAuthenticator;
use fsverity::FsverityChunkedFileReader;
use fusefs::{FileConfig, Inode};
use reader::ChunkedFileReader;

#[derive(StructOpt)]
struct Options {
    /// Mount point of AuthFS.
    #[structopt(parse(from_os_str))]
    mount_point: PathBuf,

    /// Debug only. A readonly file to be protected by fs-verity. Can be multiple.
    #[structopt(long, parse(try_from_str = parse_local_verified_file_option))]
    local_verified_file: Vec<(Inode, PathBuf, PathBuf, PathBuf)>,

    /// Debug only. An unverified read-only file. Can be multiple.
    #[structopt(long, parse(try_from_str = parse_local_unverified_file_option))]
    local_unverified_file: Vec<(Inode, PathBuf)>,
}

fn parse_local_verified_file_option(option: &str) -> Result<(Inode, PathBuf, PathBuf, PathBuf)> {
    let strs: Vec<&str> = option.split(':').collect();
    if strs.len() != 4 {
        bail!("Invalid option: {}", option);
    }
    Ok((
        strs[0].parse::<Inode>().unwrap(),
        PathBuf::from(strs[1]),
        PathBuf::from(strs[2]),
        PathBuf::from(strs[3]),
    ))
}

fn parse_local_unverified_file_option(option: &str) -> Result<(Inode, PathBuf)> {
    let strs: Vec<&str> = option.split(':').collect();
    if strs.len() != 2 {
        bail!("Invalid option: {}", option);
    }
    Ok((strs[0].parse::<Inode>().unwrap(), PathBuf::from(strs[1])))
}

fn new_config_local_verified_file(
    protected_file: &PathBuf,
    merkle_tree_dump: &PathBuf,
    signature: &PathBuf,
) -> Result<FileConfig> {
    let file = File::open(&protected_file)?;
    let file_size = file.metadata()?.len();
    let file_reader = ChunkedFileReader::new(file)?;
    let merkle_tree_reader = ChunkedFileReader::new(File::open(merkle_tree_dump)?)?;
    let authenticator = FakeAuthenticator::always_succeed();
    let mut sig = Vec::new();
    let _ = File::open(signature)?.read_to_end(&mut sig)?;
    let file_reader = FsverityChunkedFileReader::new(
        &authenticator,
        file_reader,
        file_size,
        sig,
        merkle_tree_reader,
    )?;
    Ok(FileConfig::LocalVerifiedFile(file_reader, file_size))
}

fn new_config_local_unverified_file(file_path: &PathBuf) -> Result<FileConfig> {
    let file = File::open(file_path)?;
    let file_size = file.metadata()?.len();
    let file_reader = ChunkedFileReader::new(file)?;
    Ok(FileConfig::LocalUnverifiedFile(file_reader, file_size))
}

fn prepare_file_pool(args: &Options) -> Result<HashMap<Inode, FileConfig>> {
    let mut file_pool = HashMap::new();

    for arg in &args.local_verified_file {
        let (inode, protected_file_path, merkle_tree_dump_path, signature_path) = arg;
        file_pool.insert(
            *inode,
            new_config_local_verified_file(
                protected_file_path,
                merkle_tree_dump_path,
                signature_path,
            )?,
        );
    }

    for arg in &args.local_unverified_file {
        let (inode, file_path) = arg;
        file_pool.insert(*inode, new_config_local_unverified_file(file_path)?);
    }

    Ok(file_pool)
}

fn main() -> Result<()> {
    let args = Options::from_args();
    let file_pool = prepare_file_pool(&args)?;
    fusefs::loop_forever(file_pool, &args.mount_point)?;
    Ok(())
}
