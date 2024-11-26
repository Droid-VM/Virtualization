// Copyright 2024 The Android Open Source Project
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

//! Compare device tree contents.
//! Allows skipping over fields provided.

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use argh::FromArgs;
use libfdt::Fdt;
use libfdt::FdtNode;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::path::PathBuf;

#[derive(Debug, FromArgs)]
/// Device Tree Compare arguments.
struct DtCompareArgs {
    #[argh(positional)]
    /// first device tree
    dt1: PathBuf,
    #[argh(positional)]
    /// second device tree
    dt2: PathBuf,
    #[argh(option)]
    /// list of fields that should exist but are expected to hold different values in the trees.
    ignore_value: Vec<String>,
    #[argh(option)]
    /// list of fields that will be skipped without checking values.
    skip_field: Vec<String>,
}

fn main() -> Result<()> {
    let args: DtCompareArgs = argh::from_env();
    let dt1_file = File::open(args.dt1).context("Failed to open dt1")?;
    let mut dt1: Vec<u8> = Vec::new();
    BufReader::new(dt1_file).read_to_end(&mut dt1)?;
    let dt2_file = File::open(args.dt2).context("Failed to open dt2")?;
    let mut dt2: Vec<u8> = Vec::new();
    BufReader::new(dt2_file).read_to_end(&mut dt2)?;
    compare_device_trees(dt1.as_slice(), dt2.as_slice(), args.ignore_value, args.skip_field)
}

fn compare_device_trees(
    dt1: &[u8],
    dt2: &[u8],
    ignore_value: Vec<String>,
    skip_field: Vec<String>,
) -> Result<()> {
    let fdt1 = Fdt::from_slice(dt1).unwrap();
    let fdt2 = Fdt::from_slice(dt2).unwrap();
    let ignore_set = BTreeSet::from_iter(ignore_value);
    let skip_set = BTreeSet::from_iter(skip_field);
    let root1 = fdt1.root();
    let root2 = fdt2.root();

    fn compare_props(
        root1: &FdtNode,
        root2: &FdtNode,
        ignore_set: &BTreeSet<String>,
        skip_set: &BTreeSet<String>,
    ) -> Result<()> {
        let mut prop_map: BTreeMap<String, &[u8]> = BTreeMap::new();
        for prop in root1.properties().unwrap() {
            let name = prop.name().unwrap().to_str()?;
            let value = prop.value().unwrap();
            if prop_map.insert(name.to_string(), value).is_some() {
                return Err(anyhow!("Duplicate properties detected in subnode"));
            }
        }
        for prop in root2.properties().unwrap() {
            let name = prop.name().unwrap().to_str()?;
            if skip_set.contains(name) {
                continue;
            }
            let prop_compare = match prop_map.get(name) {
                Some(val) => *val == prop.value().unwrap(),
                None => {
                    return Err(anyhow!(
                        "Extra field detected in Dt2 that is not in Dt1. Field name: {}",
                        name
                    ))
                }
            };
            // Check if value should be ignored. If yes, skip field.
            if ignore_set.contains(name) {
                continue;
            }
            if !prop_compare {
                return Err(anyhow!(
                    "Field {}'s values mismatch: {:?}, {:?}",
                    name,
                    prop_map.get(name).unwrap(),
                    prop.value().unwrap()
                ));
            }
        }
        Ok(())
    }

    fn compare_subnodes(
        node1: &FdtNode,
        node2: &FdtNode,
        ignore_set: &BTreeSet<String>,
        skip_set: &BTreeSet<String>,
    ) -> Result<()> {
        for (sn1, sn2) in node1.subnodes().unwrap().zip(node2.subnodes().unwrap()) {
            // Depth-first traversal
            compare_subnodes(&sn1, &sn2, ignore_set, skip_set)?;
            compare_props(&sn1, &sn2, ignore_set, skip_set)?;
        }
        Ok(())
    }

    compare_subnodes(&root1, &root2, &ignore_set, &skip_set)
}
