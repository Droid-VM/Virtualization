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

//! Implements converting file system to FDT blob

use anyhow::{anyhow, Context, Result};
use libfdt::Fdt;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

fn is_valid_dt_node_name_byte(byte: &u8) -> Result<()> {
    match byte {
        b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b',' | b'.' | b'_' | b'+' | b'-' | b'@' => Ok(()),
        _ => Err(anyhow!("Unsupported DT node name byte. Must only contain [0-9a-zA-Z,._+-@]")),
    }
}

fn is_valid_dt_prop_name_byte(byte: &u8) -> Result<()> {
    match byte {
        b'0'..=b'9'
        | b'a'..=b'z'
        | b'A'..=b'Z'
        | b','
        | b'.'
        | b'_'
        | b'+'
        | b'?'
        | b'#'
        | b'-' => Ok(()),
        _ => Err(anyhow!("Unsupported DT node name byte. Must only contain [0-9a-zA-Z,._+?#-]")),
    }
}

/// File system (directory) to FDT blob. File system shouldn't be changed creating the FDT.
pub fn fs_to_fdt(dir_path: &Path, fdt_file_path: &Path, fdt_max_size: usize) -> Result<()> {
    let mut data = vec![0_u8; fdt_max_size];
    let fdt = Fdt::create_empty_tree(data.as_mut_slice())
        .map_err(|e| anyhow!("Failed to create FDT, {e:?}"))?;

    let mut stack = vec![(dir_path.to_path_buf(), CString::new("/").unwrap())];

    while let Some((dir_path, fdt_path)) = stack.pop() {
        let mut node = fdt
            .node_mut(&fdt_path)
            .map_err(|e| anyhow!("Failed to write FDT, {e:?}"))?
            .ok_or_else(|| anyhow!("Internal error when writing VM reference DT"))?;

        let mut subnode_names = vec![];
        let entries =
            fs::read_dir(&dir_path).with_context(|| format!("Failed to read {dir_path:?}"))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("Failed to get an entry in {dir_path:?}"))?;
            let entry_type =
                entry.file_type().with_context(|| "Unsupported entry type, {entry:?}")?;
            let entry_name = entry.file_name(); // binding to keep name below.
            let name = CString::new(entry_name.as_bytes())
                .with_context(|| format!("Unsupported entry name for FDT, {entry:?}"))?;
            if entry_type.is_dir() {
                name.as_bytes().iter().try_for_each(is_valid_dt_node_name_byte).with_context(
                    || format!("Failed to create FDT node from {entry:?}. Unsupported character"),
                )?;

                stack.push((
                    entry.path(),
                    CString::new([fdt_path.as_bytes(), b"/", name.as_bytes()].concat()).unwrap(),
                ));

                subnode_names.push(name);
            } else if entry_type.is_file() {
                name.as_bytes().iter().try_for_each(is_valid_dt_prop_name_byte).with_context(
                    || format!("Failed to create FDT prop from {entry:?}. Unsupported character"),
                )?;

                let value = fs::read(&entry.path())?;

                node.setprop(&name, &value)
                    .map_err(|e| anyhow!("Failed to set FDT property, {e:?}"))?;
            } else {
                return Err(anyhow!("Failed to handle {entry:?}. FDT only uses file or directory"));
            }
        }
        // Note: sort() is necessary to prevent FdtError::Exists from add_subnodes().
        // FDT library may omit address in node name when comparing their name, so sort to add node
        // without address first.
        subnode_names.sort();
        let subnode_names_c_str: Vec<_> = subnode_names.iter().map(|x| x.as_c_str()).collect();
        node.add_subnodes(&subnode_names_c_str)
            .map_err(|e| anyhow!("Failed to add node, {e:?}"))?;
    }

    Ok(fs::write(fdt_file_path, fdt.as_slice())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn fdt_to_fs(fdt_file_path: &Path, dir_path: &Path) {
        let data = fs::read(fdt_file_path).unwrap();
        let fdt = Fdt::from_slice(&data).unwrap();
        let root = fdt.root().unwrap();

        let mut stack = vec![(root, dir_path.to_path_buf())];

        while let Some((node, dir_path)) = stack.pop() {
            fs::create_dir(&dir_path).unwrap();

            for subnode in node.subnodes().unwrap() {
                let name = subnode.name().unwrap().to_str().unwrap();
                stack.push((subnode, dir_path.join(name)));
            }

            for prop in node.properties().unwrap() {
                let name = prop.name().unwrap().to_str().unwrap();
                let value = prop.value().unwrap();

                let path = dir_path.join(name);
                fs::write(path, value).unwrap();
            }
        }
    }

    #[test]
    fn test_fs_to_fdt() {
        let test_dir_path = Path::new("testdata");

        let out_fdt_path = Path::new("out.dtb");
        let out_dir_path = Path::new("out");

        fs_to_fdt(test_dir_path, out_fdt_path, 1024).unwrap();
        fdt_to_fs(out_fdt_path, out_dir_path);

        let mut cmd = Command::new("diff");
        cmd.args(["-r", test_dir_path.to_str().unwrap(), out_dir_path.to_str().unwrap()]);
        let status = cmd.status().unwrap();
        assert!(status.success());
    }
}
