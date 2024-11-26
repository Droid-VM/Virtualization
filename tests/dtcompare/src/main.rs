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
use clap::Parser;
use libfdt::Fdt;
use libfdt::FdtNode;

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs::read;
use std::path::PathBuf;

#[derive(Debug, Parser)]
/// Device Tree Compare arguments.
struct DtCompareArgs {
    /// first device tree
    #[arg(long)]
    dt1: PathBuf,
    /// second device tree
    #[arg(long)]
    dt2: PathBuf,
    /// list of fields that should exist but are expected to hold different values in the trees.
    #[arg(short = 'I', long)]
    ignore_value: Vec<String>,
    /// list of fields that will ignored, whether added, removed, or changed
    #[arg(short = 'S', long)]
    skip_field: Vec<String>,
}

fn main() -> Result<()> {
    let args = DtCompareArgs::parse();
    let dt1: Vec<u8> = read(args.dt1)?;
    let dt2: Vec<u8> = read(args.dt2)?;
    compare_device_trees(dt1.as_slice(), dt2.as_slice(), args.ignore_value, args.skip_field)
}

// Compare device trees by doing a pre-order traversal of the trees.
fn compare_device_trees(
    dt1: &[u8],
    dt2: &[u8],
    ignore_value: Vec<String>,
    skip_field: Vec<String>,
) -> Result<()> {
    let fdt1 = Fdt::from_slice(dt1).context("Failed to flatten first device tree")?;
    let fdt2 = Fdt::from_slice(dt2).context("Failed to flatten second device tree")?;
    let ignore_set = BTreeSet::from_iter(ignore_value);
    let skip_set = BTreeSet::from_iter(skip_field);
    compare_subnodes(&fdt1.root(), &fdt2.root(), &ignore_set, &skip_set, &mut ["".to_string()])
}

fn compare_props(
    root1: &FdtNode,
    root2: &FdtNode,
    ignore_set: &BTreeSet<String>,
    skip_set: &BTreeSet<String>,
    path: &mut [String],
) -> Result<()> {
    let mut prop_map: BTreeMap<String, &[u8]> = BTreeMap::new();
    for prop in root1.properties().context("Error getting properties")? {
        let name =
            path.join("/") + "/" + prop.name().context("Error getting property name")?.to_str()?;
        // Do not add to prop map if skipping
        if skip_set.contains(&name) {
            continue;
        }
        let value = prop.value().context("Error getting value")?;
        if prop_map.insert(name.clone(), value).is_some() {
            return Err(anyhow!("Duplicate property detected in subnode: {}", name));
        }
    }
    for prop in root2.properties().context("Error getting properties")? {
        let name =
            path.join("/") + "/" + prop.name().context("Error getting property name")?.to_str()?;
        if skip_set.contains(&name) {
            continue;
        }
        let Some(prop1_value) = prop_map.remove(&name) else {
            return Err(anyhow!(
                "Extra field detected in Fdt2 that is not in Fdt1. Field name: {}",
                name
            ));
        };
        let prop_compare = prop1_value == prop.value().context("Error getting value")?;
        // Check if value should be ignored. If yes, skip field.
        if ignore_set.contains(&name) {
            continue;
        }
        if !prop_compare {
            return Err(anyhow!(
                "Field {}'s values mismatch: {:?}, {:?}",
                name,
                prop1_value,
                prop.value().context("Error getting value")?
            ));
        }
    }
    if !prop_map.is_empty() {
        return Err(anyhow!("Dt2 missing fields that exist in Dt1: {:?}", prop_map));
    }
    Ok(())
}

fn compare_subnodes(
    node1: &FdtNode,
    node2: &FdtNode,
    ignore_set: &BTreeSet<String>,
    skip_set: &BTreeSet<String>,
    path: &mut [String],
) -> Result<()> {
    let mut subnodes_map: BTreeMap<String, FdtNode> = BTreeMap::new();
    for subnode in node1.subnodes().context("Error getting subnodes of first FDT")? {
        let name = path.join("/")
            + "/"
            + subnode.name().context("Error getting property name")?.to_str()?;
        // Do not add to subnode map if skipping
        if skip_set.contains(&name) {
            continue;
        }
        if subnodes_map.insert(name.clone(), subnode).is_some() {
            return Err(anyhow!("Duplicate subnodes detected: {}", name));
        }
    }
    for sn2 in node2.subnodes().context("Error getting subnodes of second FDT")? {
        let name =
            path.join("/") + "/" + sn2.name().context("Error getting subnode name")?.to_str()?;
        let sn1 = subnodes_map.remove(&name);
        match sn1 {
            Some(sn) => {
                compare_props(&sn, &sn2, ignore_set, skip_set, &mut [name.clone()])?;
                compare_subnodes(&sn, &sn2, ignore_set, skip_set, &mut [name.clone()])?;
            }
            None => return Err(anyhow!("Fdt1 missing node {} from Fdt2", name)),
        }
    }
    if !subnodes_map.is_empty() {
        return Err(anyhow!("Fdt2 missing nodes that exist in Fdt1: {:?}", subnodes_map));
    }
    Ok(())
}
