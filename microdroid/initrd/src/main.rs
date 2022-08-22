/*
 * Copyright (C) 2022 The Android Open Source Project
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

//! Append bootconfig in inird image
use anyhow::Result;

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
struct Args {
    /// Initrd (without bootconfig)
    #[structopt(parse(from_os_str))]
    initrd_path: PathBuf,
    /// bootconfig
    #[structopt(parse(from_os_str))]
    bootconfig_path: PathBuf,
    /// Output
    #[structopt(parse(from_os_str))]
    initrd_with_bootconfig_path: PathBuf,
}
const ZEROS: [u8; 4] = [0u8; 4_usize];

fn get_checksum(file_path: &PathBuf) -> Result<u32> {
    // A buffer of exact 1 byte
    let mut buf: [u8; 1] = [0; 1];
    let mut checksum: u32 = 0;
    let file = File::open(file_path)?;

    while (&file).read(&mut buf)? > 0 {
        checksum += buf[0] as u32;
    }
    Ok(checksum)
}

// Bootconfig is attached to the initrd in the following way:
// [initrd][bootconfig][padding][size(le32)][checksum(le32)][#BOOTCONFIG\n]
fn attach_bootconfig(
    initrd_path: PathBuf,
    bootconfig_path: PathBuf,
    initrd_with_bootconfig_path: PathBuf,
) -> Result<()> {
    std::fs::copy(&initrd_path, &initrd_with_bootconfig_path)?;
    let initrd_size: usize = std::fs::metadata(initrd_path)?.len().try_into().unwrap();
    let mut bootconfig_file = File::open(&bootconfig_path)?;
    let mut initrd_with_bootconfig_file =
        OpenOptions::new().append(true).open(&initrd_with_bootconfig_path)?;
    let bootconfig_size: usize = bootconfig_file.metadata()?.len().try_into().unwrap();

    let _copied = std::io::copy(&mut bootconfig_file, &mut initrd_with_bootconfig_file)?;
    let padding_size = 4 - (initrd_size + bootconfig_size) % 4;
    initrd_with_bootconfig_file.write_all(&ZEROS[..padding_size])?;
    initrd_with_bootconfig_file
        .write_all(&((padding_size + bootconfig_size) as u32).to_le_bytes())?;
    initrd_with_bootconfig_file.write_all(&get_checksum(&bootconfig_path)?.to_le_bytes())?;
    initrd_with_bootconfig_file.write_all(b"#BOOTCONFIG\n")?;
    initrd_with_bootconfig_file.flush()?;
    Ok(())
}

fn try_main() -> Result<()> {
    let args = Args::from_args_safe()?;
    println!("Running initrd_bootconfig with arguments {:?}", args);
    attach_bootconfig(args.initrd_path, args.bootconfig_path, args.initrd_with_bootconfig_path)?;
    Ok(())
}

fn main() {
    if let Err(e) = try_main() {
        println!("failed with {:?}", e);
        std::process::exit(1);
    }
}
