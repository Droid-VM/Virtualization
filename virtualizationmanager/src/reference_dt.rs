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

//! Functions for VM reference DT

use anyhow::{anyhow, Result};
use fsfdt::FsFdt;
use libfdt::Fdt;
use std::fs;
use std::fs::File;
use std::path::Path;

const VM_REFERENCE_DT_ON_HOST_PATH: &str = "/proc/device-tree/avf/reference";
const VM_REFERENCE_DT_NAME: &str = "vm_reference_dt.dtb";
const VM_REFERENCE_DT_MAX_SIZE: usize = 2000;

// Parses to VM reference if exists.
// TODO(b/318431695): Allow to parse from custom VM reference DT
pub(crate) fn parse_reference_dt(out_dir: &Path) -> Result<Option<File>> {
    parse_reference_dt_internal(
        Path::new(VM_REFERENCE_DT_ON_HOST_PATH),
        &out_dir.join(VM_REFERENCE_DT_NAME),
    )
}

fn parse_reference_dt_internal(dir_path: &Path, fdt_path: &Path) -> Result<Option<File>> {
    if dir_path.try_exists()? {
        let mut data = vec![0_u8; VM_REFERENCE_DT_MAX_SIZE];
        let fdt = Fdt::from_fs(dir_path, &mut data)?;
        fdt.pack().map_err(|e| anyhow!("Failed to pack VM reference DT, {e:?}"))?;
        fs::write(fdt_path, fdt.as_slice())?;
        Ok(Some(File::open(fdt_path)?))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libfdt::Fdt;
    use std::fs;
    use std::io::Read;

    #[test]
    fn test_parse_reference_dt_from_empty_dir() {
        let dir_path = Path::new("empty_dir");
        let fdt_path = Path::new("test.dtb");

        fs::create_dir(dir_path).unwrap();

        let fdt_file = parse_reference_dt_internal(dir_path, fdt_path).unwrap();

        assert!(fdt_file.is_some());

        let mut fdt_data = Vec::new();
        fdt_file.unwrap().read_to_end(&mut fdt_data).unwrap();

        let fdt_data_from_path = fs::read(fdt_path).unwrap();
        assert_eq!(fdt_data, fdt_data_from_path);

        let fdt = Fdt::from_slice(&fdt_data).unwrap();

        let root = fdt.root().unwrap();
        let fdt_all_nodes: Vec<_> = root.descendants().collect();

        assert_eq!(fdt_all_nodes, Vec::new());
    }

    #[test]
    fn test_parse_reference_dt_from_empty_reference() {
        let fdt_file = parse_reference_dt_internal(
            Path::new("/this/path/would/not/exists"),
            Path::new("test.dtb"),
        )
        .unwrap();

        assert!(fdt_file.is_none());
    }
}
