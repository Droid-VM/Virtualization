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
use libfdt::Fdt;
use libfdt::FdtNode;

use std::collections::BTreeSet;
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;

fn main() -> Result<()> {
    let mut args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err(anyhow!(
            "Incorrect number of arguments passed to dtcompare.\n
                           Usage:\n
                           dtcompare path/to/dt1.dtb path/to/dt2.dtb\n
                           Optional: add a list of fields to ignore.\n
                           Ex: dtcompare path/to/dt1.dtb path/to/dt2.dtb FIELD1 FIELD2 FIELD3 ...
                           "
        ));
    }
    // Safe to unwrap as it has been validated there are more than 2 arguments.
    let dt1_file = File::open(args.pop().unwrap()).context("Failed to open dt1")?;
    let mut dt1: Vec<u8> = Vec::new();
    BufReader::new(dt1_file).read_to_end(&mut dt1)?;
    let dt2_file = File::open(args.pop().unwrap()).context("Failed to open dt2")?;
    let mut dt2: Vec<u8> = Vec::new();
    BufReader::new(dt2_file).read_to_end(&mut dt2)?;
    compare_device_trees(dt1.as_slice(), dt2.as_slice(), args)
}

fn compare_device_trees(dt1: &[u8], dt2: &[u8], ignore_value: Vec<String>) -> Result<()> {
    let fdt1 = Fdt::from_slice(dt1).unwrap();
    let fdt2 = Fdt::from_slice(dt2).unwrap();
    let ignore_set = BTreeSet::from_iter(ignore_value);
    let root1 = fdt1.root();
    let root2 = fdt2.root();

    fn compare_props(
        root1: &FdtNode,
        root2: &FdtNode,
        ignore_set: &BTreeSet<String>,
    ) -> Result<()> {
        for (prop1, prop2) in root1.properties().unwrap().zip(root2.properties().unwrap()) {
            // Only need to validate 1 prop. If they are different, assertion fails.
            let name = prop1.name().unwrap().to_owned();
            if ignore_set.contains(name.to_str()?) {
                // Skip checking the field
                continue;
            }
            if prop1.value() != prop2.value() {
                return Err(anyhow!("DT differences: prop1: {:?}, prop2: {:?}", prop1, prop2));
            }
        }
        Ok(())
    }

    fn compare_subnodes(
        node1: &FdtNode,
        node2: &FdtNode,
        ignore_set: &BTreeSet<String>,
    ) -> Result<()> {
        for (sn1, sn2) in node1.subnodes().unwrap().zip(node2.subnodes().unwrap()) {
            // Depth-first traversal
            compare_subnodes(&sn1, &sn2, ignore_set)?;
            compare_props(&sn1, &sn2, ignore_set)?;
        }
        Ok(())
    }

    compare_subnodes(&root1, &root2, &ignore_set)
}
