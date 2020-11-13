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

//! This crate provides a FUSE-based, non-generic filesystem that I/O is authenticated. This
//! filesystem assumes the storage layer is not trusted, e.g. file is provided by an untrusted VM,
//! and the content can't be simply trusted. The filesystem can use its public key to verify a
//! (read-only) file against its associated fs-verity signature by a trusted party. With the Merkle
//! tree, each read of file block can be verified individually.
//!
//! The implementation is NOT finished.

use anyhow::Result;
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
use fusefs::FileConfig;
use reader::ChunkedFileReader;

// TODO fd. path is for debugging
#[derive(StructOpt)]
struct Opt {
    /// The file path to be protected by fs-verity.
    #[structopt(parse(from_os_str))]
    protected_file_path: PathBuf,
    /// The file path of a Merkle tree dump of fs-verity.
    #[structopt(parse(from_os_str))]
    merkle_tree_dump_path: PathBuf,
    /// The file path of an fs-verity signature.
    #[structopt(parse(from_os_str))]
    signature_path: PathBuf,
}

fn main() -> Result<()> {
    let args = Opt::from_args();

    let mut dir_tree = HashMap::new();
    let mut inode = 2;

    {
        let file = File::open(&args.protected_file_path)?;
        let file_size = file.metadata()?.len();
        let file_reader = ChunkedFileReader::new(file)?;

        dir_tree.insert(inode, FileConfig::FsverityFile(file_reader, file_size));
        inode += 1;
    }

    {
        let authenticator = FakeAuthenticator::always_succeed();
        let file = File::open(&args.protected_file_path)?;
        let file_size = file.metadata()?.len();
        let file_reader = ChunkedFileReader::new(file)?;
        let merkle_tree_reader = ChunkedFileReader::new(File::open(&args.merkle_tree_dump_path)?)?;
        let mut sig = Vec::new();
        let _ = File::open(&args.signature_path)?.read_to_end(&mut sig)?;
        let file_reader = FsverityChunkedFileReader::new(
            &authenticator,
            file_reader,
            file_size,
            sig,
            merkle_tree_reader,
        )?;

        dir_tree.insert(inode, FileConfig::UnverifiedFile(file_reader, file_size));
        inode += 1;
    }

    fusefs::run(dir_tree);
    Ok(())
}
