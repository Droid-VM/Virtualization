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

//! Append bootconfig to initrd image
use anyhow::Result;

use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use structopt::StructOpt;

const FOOTER_ALIGNMENT: usize = 4;

#[derive(StructOpt, Debug)]
struct Args {
    /// Initrd (without bootconfig)
    #[structopt(parse(from_os_str))]
    initrd: PathBuf,
    /// Bootconfig
    #[structopt(parse(from_os_str))]
    bootconfig: PathBuf,
    /// Output
    #[structopt(parse(from_os_str))]
    output: PathBuf,
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
fn attach_bootconfig(initrd: PathBuf, bootconfig: PathBuf, output: PathBuf) -> Result<()> {
    let mut output_file = File::create(&output)?;
    let mut initrd_file = File::open(&initrd)?;
    let mut bootconfig_file = File::open(&bootconfig)?;
    let initrd_size: usize = initrd_file.metadata()?.len().try_into()?;
    let bootconfig_size: usize = bootconfig_file.metadata()?.len().try_into()?;

    std::io::copy(&mut initrd_file, &mut output_file)?;
    std::io::copy(&mut bootconfig_file, &mut output_file)?;

    let padding_size: usize = FOOTER_ALIGNMENT - (initrd_size + bootconfig_size) % FOOTER_ALIGNMENT;
    output_file.write_all(&ZEROS[..padding_size])?;
    output_file.write_all(&((padding_size + bootconfig_size) as u32).to_le_bytes())?;
    output_file.write_all(&get_checksum(&bootconfig)?.to_le_bytes())?;
    output_file.write_all(b"#BOOTCONFIG\n")?;
    output_file.flush()?;
    Ok(())
}

fn try_main() -> Result<()> {
    let args = Args::from_args_safe()?;
    attach_bootconfig(args.initrd, args.bootconfig, args.output)?;
    Ok(())
}

fn main() {
    try_main().unwrap()
}
