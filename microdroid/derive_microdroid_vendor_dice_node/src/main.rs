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

//! Derives microdroid vendor dice node.

use anyhow::{bail, Context, Error};
use ciborium::{cbor, Value};
use clap::Parser;
use coset::CborSerializable;
use dice_driver::DiceDriver;
use diced_open_dice::{OwnedDiceArtifacts, HIDDEN_SIZE};
use dm::util::blkgetsize64;
use openssl::sha::Sha512;
use std::fs::{read_link, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use vbmeta::VbMetaImage;

const AVF_STRICT_BOOT: &str = "/proc/device-tree/chosen/avf,strict-boot";

#[derive(Parser)]
struct Args {
    /// Path to the dice driver (e.g. /dev/open-dice0)
    #[arg(long)]
    dice_driver: PathBuf,
    /// Path to the microdroid-vendor.img disk image.
    #[arg(long)]
    microdroid_vendor_disk_image: PathBuf,
    /// File to save resulting dice chain to.
    #[arg(long)]
    output: PathBuf,
}

// TODO(ioffe): move to a library to reuse same code here, in microdroid_manager and in
// first_stage_init.
fn is_strict_boot() -> bool {
    Path::new(AVF_STRICT_BOOT).exists()
}

// TODO(ioffe): also incldue the rollback index.
fn build_descriptor() -> Result<Vec<u8>, Error> {
    let mut map = Vec::new();
    map.push((cbor!(-70002)?, cbor!("Microdroid vendor")?));
    Ok(Value::Map(map).to_vec()?)
}

fn find_root_digest(vbmeta: &VbMetaImage) -> Result<Option<Vec<u8>>, Error> {
    for descriptor in vbmeta.descriptors()?.iter() {
        if let vbmeta::Descriptor::Hashtree(_) = descriptor {
            let root_digest = hex::encode(descriptor.to_hashtree()?.root_digest());
            return Ok(Some(root_digest.as_bytes().to_vec()));
        }
    }
    Ok(None)
}

fn dice_derivation(dice: DiceDriver, vbmeta: &VbMetaImage) -> Result<OwnedDiceArtifacts, Error> {
    let mut code_hash = Sha512::new();
    let mut authority_hash = Sha512::new();
    if let Some(pubkey) = vbmeta.public_key() {
        authority_hash.update(pubkey);
    } else {
        bail!("no public key");
    }
    // TODO(ioffe): is this the correct one?
    if let Some(root_digest) = find_root_digest(vbmeta)? {
        code_hash.update(root_digest.as_ref());
    } else {
        bail!("no hashtree");
    }
    let desc = build_descriptor()?;
    // TODO(ioffe): we also need to pass is_debuggable here
    // TODO(ioffe): what to do with hidden?
    let hidden = [0; HIDDEN_SIZE];
    dice.derive(code_hash.finish(), &desc, authority_hash.finish(), false, hidden)
}

fn extract_vbmeta(block_dev: &Path) -> Result<VbMetaImage, Error> {
    let size = blkgetsize64(block_dev).context("blkgetsize64  failed")?;
    let file = File::open(block_dev).context("open failed")?;
    let vbmeta = VbMetaImage::verify_reader_region(file, 0, size)?;
    Ok(vbmeta)
}

fn try_main() -> Result<(), Error> {
    let args = Args::parse();
    let dice =
        DiceDriver::new(&args.dice_driver, is_strict_boot()).context("Failed to load DICE")?;
    let path = read_link(args.microdroid_vendor_disk_image).context("failed to read symlink")?;
    eprintln!("[ioffe] read symlink : {:?}", path);
    let vbmeta = extract_vbmeta(&path)?;
    let _dice_artifacts = dice_derivation(dice, &vbmeta)?;
    let mut file = File::create(&args.output)?;
    file.write_all(b"dice_artifacts")?;
    Ok(())
}

fn main() {
    if let Err(e) = try_main() {
        eprintln!("failed with {:?}", e);
        std::process::exit(1);
    }
}
