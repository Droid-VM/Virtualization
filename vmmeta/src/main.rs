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

//! Tool for handling VMMeta blobs.

use anyhow::{bail, ensure, Result};
use apexutil::get_payload_vbmeta_digest;
use apkverify::get_signing_block_digest;
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use serde::ser::Serialize;
use std::fmt::Write;
use vmmeta::{ApkDescriptor, Descriptor, ResourceIdentifier, VbMetaDescriptor, VmMeta};

fn decode_hex(s: &str) -> Result<Vec<u8>, std::num::ParseIntError> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16)).collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{:02x}", b).expect("Unable to hex encode");
    }
    s
}

fn get_vbmeta_digest(file: &str) -> Result<Vec<u8>> {
    // TODO: can this be done with libavb_bindgen instead?
    let output = std::process::Command::new("avbtool")
        .args(["calculate_vbmeta_digest", "--hash_algorithm", "sha512", "--image", file])
        .output()?;
    ensure!(output.status.success());
    let vbmeta_digest_hexstring = std::str::from_utf8(&output.stdout)?.trim();
    let vbmeta_digest = decode_hex(vbmeta_digest_hexstring)?;
    Ok(vbmeta_digest)
}

/// Iterate over a set of argument values, that could be empty, and follow the
/// <namespace>:<name>:<file> format valiated by namespace_name_file().
///
/// It's just a zip and a map but with some logic to turn a lack of values for
/// an argument into an empty iterator.
struct NamespaceNameFileIterator<'a> {
    indices: Option<clap::Indices<'a>>,
    values: Option<clap::Values<'a>>,
}

impl<'a> NamespaceNameFileIterator<'a> {
    fn new(args: &'a ArgMatches, name: &'a str) -> Self {
        NamespaceNameFileIterator { indices: args.indices_of(name), values: args.values_of(name) }
    }
}

impl<'a> Iterator for NamespaceNameFileIterator<'a> {
    type Item = (usize, &'a str, &'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        match (self.indices.as_mut(), self.values.as_mut()) {
            (Some(indices), Some(values)) => match (indices.next(), values.next()) {
                (Some(index), Some(value)) => {
                    let mut split = value.split(':');
                    match (split.next(), split.next(), split.next()) {
                        (Some(namespace), Some(name), Some(file)) => {
                            Some((index, namespace, name, file))
                        }
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }
}

fn create(args: &ArgMatches) -> Result<()> {
    // Store descriptors in the order they were given in the arguments
    // TODO: instead, group them by namespace?
    let mut descriptors = std::collections::BTreeMap::new();
    for (i, namespace, name, file) in NamespaceNameFileIterator::new(args, "vbmeta") {
        descriptors.insert(
            i,
            Descriptor::VbMeta(VbMetaDescriptor {
                resource: ResourceIdentifier {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                },
                vbmeta_digest: get_vbmeta_digest(file)?,
            }),
        );
    }
    for (i, namespace, name, file) in NamespaceNameFileIterator::new(args, "apk") {
        descriptors.insert(
            i,
            Descriptor::Apk(ApkDescriptor {
                resource: ResourceIdentifier {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                },
                signing_block_digest: get_signing_block_digest(file)?.to_vec(),
            }),
        );
    }
    for (i, namespace, name, file) in NamespaceNameFileIterator::new(args, "apex-payload") {
        descriptors.insert(
            i,
            Descriptor::VbMeta(VbMetaDescriptor {
                resource: ResourceIdentifier {
                    namespace: namespace.to_string(),
                    name: name.to_string(),
                },
                vbmeta_digest: get_payload_vbmeta_digest(file)?,
            }),
        );
    }

    let mut vmmeta = VmMeta::new();
    descriptors.into_values().for_each(|d| vmmeta.add_descriptor(d));

    let mut bytes = Vec::new();
    vmmeta.serialize(
        &mut serde_cbor::Serializer::new(&mut serde_cbor::ser::IoWrite::new(&mut bytes))
            .packed_format()
            .legacy_enums(),
    )?;
    std::fs::write(args.value_of("file").unwrap(), &bytes)?;
    Ok(())
}

fn dump(args: &ArgMatches) -> Result<()> {
    let file = std::fs::read(args.value_of("file").unwrap())?;
    let vmmeta: VmMeta = serde_cbor::from_slice(&file)?;
    println!("Descriptors:");
    for descriptor in vmmeta.descriptors() {
        match descriptor {
            Descriptor::VbMeta(data) => {
                println!("  VBMeta descriptor:");
                println!("    namespace:             {}", data.resource.namespace);
                println!("    name:                  {}", data.resource.name);
                println!("    vbmeta digest:         {}", encode_hex(&data.vbmeta_digest))
            },
            Descriptor::Apk(data) => {
                println!("  APK descriptor:");
                println!("    namespace:             {}", data.resource.namespace);
                println!("    name:                  {}", data.resource.name);
                println!("    signing block digest:  {}", encode_hex(&data.signing_block_digest))
            },
        }
    }
    Ok(())
}

fn namespace_name_file(v: String) -> std::result::Result<(), String> {
    if v.split(':').count() != 3 {
        Err(String::from("<namespace>:<name>:<file> format required"))
    } else {
        Ok(())
    }
}

/*
*
*vmmetatool create \
   --vbmeta pvmfw:preload:u-boot.bin \
   --vbmeta uboot:env_vbmeta:disk1/vbmeta.imb \
   --vbmeta uboot:vbmeta:micordoid/vbmeta.img \
   --apk microdroid:payload:compos.apk \
   --apk microdroid:extra_apk:extra_apk.apk \
   --apex-payload microdroid:art_apex:art.apex
*/
fn main() -> Result<()> {
    let app = App::new("vmmetatool").setting(AppSettings::SubcommandRequiredElseHelp).subcommand(
        SubCommand::with_name("create")
            .setting(AppSettings::ArgRequiredElseHelp)
            .arg(Arg::with_name("file").takes_value(true))
            .arg(
                Arg::with_name("vbmeta")
                    .long("vbmeta")
                    .takes_value(true)
                    .multiple(true)
                    .validator(namespace_name_file),
            )
            .arg(
                Arg::with_name("apk")
                    .long("apk")
                    .takes_value(true)
                    .multiple(true)
                    .validator(namespace_name_file),
            )
            .arg(
                Arg::with_name("apex-payload")
                    .long("apex-payload")
                    .takes_value(true)
                    .multiple(true)
                    .validator(namespace_name_file),
            ),
    ).subcommand(
        SubCommand::with_name("dump")
            .setting(AppSettings::ArgRequiredElseHelp)
            .arg(Arg::with_name("file").takes_value(true))
    );
    // TODO: ArgGroup for vbmeta, apk etc.

    let args = app.get_matches();
    match args.subcommand() {
        ("create", Some(sub_args)) => create(sub_args),
        ("dump", Some(sub_args)) => dump(sub_args),
        _ => bail!("Invalid arguments"),
    }
}
