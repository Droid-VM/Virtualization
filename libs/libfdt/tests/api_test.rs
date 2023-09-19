/*
 * Copyright (C) 2023 The Android Open Source Project
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

//! Integration tests of the library libfdt.

use libfdt::{Fdt, FdtError, OwnedFdt};
use std::ffi::CStr;
use std::fs;
use std::ops::Range;

const TEST_TREE_WITH_ONE_MEMORY_RANGE_PATH: &str = "data/test_tree_one_memory_range.dtb";
const TEST_TREE_WITH_MULTIPLE_MEMORY_RANGES_PATH: &str =
    "data/test_tree_multiple_memory_ranges.dtb";
const TEST_TREE_WITH_EMPTY_MEMORY_RANGE_PATH: &str = "data/test_tree_empty_memory_range.dtb";
const TEST_TREE_WITH_NO_MEMORY_NODE_PATH: &str = "data/test_tree_no_memory_node.dtb";
const TEST_TREE_OVERLAY_PATH: &str = "data/test_tree_overlay.dtbo";

macro_rules! cstr {
    ($str:literal) => {{
        CStr::from_bytes_with_nul(concat!($str, "\0").as_bytes()).unwrap()
    }};
}

#[test]
fn retrieving_memory_from_fdt_with_one_memory_range_succeeds() {
    let data = fs::read(TEST_TREE_WITH_ONE_MEMORY_RANGE_PATH).unwrap();
    let fdt = Fdt::from_slice(&data).unwrap();

    const EXPECTED_FIRST_MEMORY_RANGE: Range<usize> = 0..256;
    let mut memory = fdt.memory().unwrap();
    assert_eq!(memory.next(), Some(EXPECTED_FIRST_MEMORY_RANGE));
    assert!(memory.next().is_none());
    assert_eq!(fdt.first_memory_range(), Ok(EXPECTED_FIRST_MEMORY_RANGE));
}

#[test]
fn retrieving_memory_from_fdt_with_multiple_memory_ranges_succeeds() {
    let data = fs::read(TEST_TREE_WITH_MULTIPLE_MEMORY_RANGES_PATH).unwrap();
    let fdt = Fdt::from_slice(&data).unwrap();

    const EXPECTED_FIRST_MEMORY_RANGE: Range<usize> = 0..256;
    const EXPECTED_SECOND_MEMORY_RANGE: Range<usize> = 512..1024;
    let mut memory = fdt.memory().unwrap();
    assert_eq!(memory.next(), Some(EXPECTED_FIRST_MEMORY_RANGE));
    assert_eq!(memory.next(), Some(EXPECTED_SECOND_MEMORY_RANGE));
    assert!(memory.next().is_none());
    assert_eq!(fdt.first_memory_range(), Ok(EXPECTED_FIRST_MEMORY_RANGE));
}

#[test]
fn retrieving_first_memory_from_fdt_with_empty_memory_range_fails() {
    let data = fs::read(TEST_TREE_WITH_EMPTY_MEMORY_RANGE_PATH).unwrap();
    let fdt = Fdt::from_slice(&data).unwrap();

    let mut memory = fdt.memory().unwrap();
    assert!(memory.next().is_none());
    assert_eq!(fdt.first_memory_range(), Err(FdtError::NotFound));
}

#[test]
fn retrieving_memory_from_fdt_with_no_memory_node_fails() {
    let data = fs::read(TEST_TREE_WITH_NO_MEMORY_NODE_PATH).unwrap();
    let fdt = Fdt::from_slice(&data).unwrap();

    assert_eq!(fdt.memory().unwrap_err(), FdtError::NotFound);
    assert_eq!(fdt.first_memory_range(), Err(FdtError::NotFound));
}

#[test]
fn node_name() {
    let data = fs::read(TEST_TREE_WITH_NO_MEMORY_NODE_PATH).unwrap();
    let fdt = Fdt::from_slice(&data).unwrap();

    let root = fdt.root().unwrap();
    assert_eq!(root.name().unwrap().to_str().unwrap(), "");

    let chosen = fdt.chosen().unwrap().unwrap();
    assert_eq!(chosen.name().unwrap().to_str().unwrap(), "chosen");

    let nested_node_path = CStr::from_bytes_with_nul(b"/cpus/PowerPC,970@0\0").unwrap();
    let nested_node = fdt.node(nested_node_path).unwrap().unwrap();
    assert_eq!(nested_node.name().unwrap().to_str().unwrap(), "PowerPC,970@0");
}

#[test]
fn node_subnode() {
    let data = fs::read(TEST_TREE_WITH_NO_MEMORY_NODE_PATH).unwrap();
    let fdt = Fdt::from_slice(&data).unwrap();
    let root = fdt.root().unwrap();
    let expected: Vec<&str> = vec!["cpus", "randomnode", "chosen"];

    for (node, name) in root.subnode().unwrap().zip(expected) {
        assert_eq!(node.name().unwrap().to_str().unwrap(), name);
    }
}

#[test]
fn owned_fdt() {
    let data = fs::read(TEST_TREE_OVERLAY_PATH).unwrap();
    let owned_fdt = OwnedFdt::from_overlay_onto_new_fdt(Some(&data)).unwrap();
    let fdt = owned_fdt.as_fdt();
    let root = fdt.root().unwrap();
    assert_eq!(123, root.getprop_u32(cstr!("version")).unwrap().unwrap());

    let node = fdt.node(cstr!("/node@test")).unwrap().unwrap();
    assert_eq!(cstr!("hello"), node.getprop_str(cstr!("type")).unwrap().unwrap());
    assert!(fdt.node(cstr!("/node@overlay")).unwrap().is_none());
}
