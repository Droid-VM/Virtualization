// Copyright 2022, The Android Open Source Project
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

//! Attach/Detach bootconfigs to initrd image
use anyhow::{bail, Result};
use clap::Parser;
use std::cmp::min;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

const FOOTER_ALIGNMENT: usize = 4;
const ZEROS: [u8; 4] = [0u8; 4_usize];
const BOOTCONFIG_MAGIC: &str = "#BOOTCONFIG\n";
const BUFFSIZE: usize = 1024;

#[derive(Parser, Debug)]
enum Opt {
    // Append bootconfig to initrd image
    AppendBootconfig {
        /// Initrd (without bootconfig)
        initrd: PathBuf,
        /// Bootconfig
        bootconfigs: Vec<PathBuf>,
        /// Output
        #[clap(long = "output")]
        output: PathBuf,
    },

    // Detach the initrd & bootconfigs - this is required for cases when we replace bootconfig values in sign_virt apex
    DetachBootconfig {
        /// Initrd (with bootconfig)
        initrd_with_bootconfig: PathBuf,
        /// Initrd (without bootconfig)
        initrd: PathBuf,
        /// Bootconfig
        bootconfig: PathBuf,
    },
}

fn get_checksum(file_path: &PathBuf) -> Result<u32> {
    File::open(file_path)?.bytes().map(|x| Ok(x? as u32)).sum()
}

// Note attaching & then detaching bootconfig can lead to extra padding in bootconfigs
fn detach_bootconfig(initrd_bc: PathBuf, initrd: PathBuf, bootconfig: PathBuf) -> Result<()> {
    let mut initrd_bc = File::open(&initrd_bc)?;
    let mut bootconfig = OpenOptions::new().write(true).open(&bootconfig)?;
    let mut initrd = OpenOptions::new().write(true).open(&initrd)?;
    let initrd_bc_size: usize = initrd_bc.metadata()?.len().try_into()?;
    let mut buf = vec![0; BUFFSIZE];

    initrd_bc.seek(SeekFrom::End(-(BOOTCONFIG_MAGIC.len() as i64)))?;
    let mut magic_buf = [0; BOOTCONFIG_MAGIC.len()];
    initrd_bc.read_exact(&mut magic_buf)?;
    if magic_buf != BOOTCONFIG_MAGIC.as_bytes() {
        bail!("BOOTCONFIG_MAGIC not found in initrd. Bootconfigs might not be attached correctly");
    }

    initrd_bc.seek(SeekFrom::End(-(BOOTCONFIG_MAGIC.len() as i64 + 8)))?;
    initrd_bc.read_exact(&mut buf[..4])?;
    let bc_size: usize =
        u32::from_le_bytes(buf[..4].try_into().expect("slice with incorrect length")) as usize;

    let initrd_size: usize = initrd_bc_size - bc_size - 8 - BOOTCONFIG_MAGIC.len();

    initrd_bc.seek(SeekFrom::Start(0))?;
    let mut copied: usize = 0;
    while copied < initrd_size {
        let n = min(initrd_size - copied, BUFFSIZE);
        initrd_bc.read_exact(&mut buf[..n])?;
        initrd.write_all(&buf[..n])?;
        copied += n;
    }

    copied = 0;
    while copied < bc_size {
        let n = min(bc_size - copied, BUFFSIZE);
        initrd_bc.read_exact(&mut buf[..n])?;
        bootconfig.write_all(&buf[..n])?;
        copied += n;
    }

    Ok(())
}

// Bootconfig is attached to the initrd in the following way:
// [initrd][bootconfig][padding][size(le32)][checksum(le32)][#BOOTCONFIG\n]
fn attach_bootconfig(initrd: PathBuf, bootconfigs: Vec<PathBuf>, output: PathBuf) -> Result<()> {
    let mut output_file = File::create(&output)?;
    let mut initrd_file = File::open(&initrd)?;
    let initrd_size: usize = initrd_file.metadata()?.len().try_into()?;
    let mut bootconfig_size: usize = 0;
    let mut checksum: u32 = 0;

    std::io::copy(&mut initrd_file, &mut output_file)?;
    for bootconfig in bootconfigs {
        let mut bootconfig_file = File::open(&bootconfig)?;
        std::io::copy(&mut bootconfig_file, &mut output_file)?;
        bootconfig_size += bootconfig_file.metadata()?.len() as usize;
        checksum += get_checksum(&bootconfig)?;
    }

    let padding_size: usize =
        (FOOTER_ALIGNMENT - (initrd_size + bootconfig_size) % FOOTER_ALIGNMENT) % 4;
    output_file.write_all(&ZEROS[..padding_size])?;
    output_file.write_all(&((padding_size + bootconfig_size) as u32).to_le_bytes())?;
    output_file.write_all(&checksum.to_le_bytes())?;
    output_file.write_all(BOOTCONFIG_MAGIC.as_bytes())?;
    output_file.flush()?;
    Ok(())
}

fn try_main() -> Result<()> {
    let args = Opt::parse();
    match args {
        Opt::AppendBootconfig { initrd, bootconfigs, output } => {
            attach_bootconfig(initrd, bootconfigs, output)?
        }
        Opt::DetachBootconfig { initrd_with_bootconfig, initrd, bootconfig } => {
            detach_bootconfig(initrd_with_bootconfig, initrd, bootconfig)?
        }
    };
    Ok(())
}

fn main() {
    try_main().unwrap()
}
