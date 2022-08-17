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

//!  Lets you dynamically append the right bootconfig to the init rd cpio image
use anyhow::Result;
use log::{error, info};

// use protobuf::Message;
// use std::convert::TryInto;
// Todo: do BufReader instead
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
// use std::num::NonZeroU8;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
struct Args {
    /// Initrd (without bootconfig)
    #[structopt(parse(from_os_str))]
    initrd_path: PathBuf,
    /// Initrd (without bootconfig)
    #[structopt(parse(from_os_str))]
    bootconfig_path: PathBuf,
    /// Initrd (without bootconfig)
    #[structopt(parse(from_os_str))]
    initrd_with_bootconfig_path: PathBuf,

    /// Enable debugging in the tool.
    #[structopt(short, long)]
    debug: bool,
}
const ZEROS: [u8; 4] = [0u8; 4_usize];

fn get_checksum(file_path: &PathBuf) -> Result<u32> {
    // A buffer of extact 1 byte
    let mut buf: [u8; 1] = [0; 1];
    let mut checksum: u32 = 0;
    let file = File::open(file_path)?;

    while (&file).read(&mut buf)? > 0 {
        checksum += buf[0] as u32;
    }

    Ok(checksum)
}

// Todo(b/240235430): See if we can use the tools/bootconfig/bootconfig
// to add the bootconfig.
// Bootconfig is attached in the following way:
// [initrd][bootconfig][padding][size(le32)][checksum(le32)][#BOOTCONFIGn]
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

    info!(
        "Size of all inital initrd: {}, bootconfig: {}, final initrd size: {}",
        initrd_size,
        bootconfig_size,
        initrd_with_bootconfig_file.metadata()?.len(),
    );
    Ok(())
}

fn try_main() -> Result<()> {
    let args = Args::from_args_safe()?;
    println!("Running with arguments {:?}", args);
    let log_level = if args.debug { log::Level::Debug } else { log::Level::Info };

    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("initrd_attach_bootconfig")
            .with_min_level(log_level),
    );

    attach_bootconfig(args.initrd_path, args.bootconfig_path, args.initrd_with_bootconfig_path)?;
    Ok(())
}

// Todo: make it pass an open fd to bootconfig_path instead
fn main() {
    info!("Attaching bootconfig to initrd...");
    if let Err(e) = try_main() {
        error!("failed with {:?}", e);
        std::process::exit(1);
    }
}

// Todo(write a test)
