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

//! This module support creating AFV related overlays, that can then be appended to DT by VM.

use anyhow::{anyhow, Result};
use cstr::cstr;
use fsfdt::FsFdt;
use libfdt::Fdt;
use std::ffi::CStr;
use std::path::Path;

pub(crate) const VM_REFERENCE_DT_ON_HOST_PATH: &str = "/proc/device-tree/avf/reference";
pub(crate) const VM_DT_OVERLAY_PATH: &str = "vm_dt_overlay.dtbo";
pub(crate) const VM_DT_OVERLAY_MAX_SIZE: usize = 2000;

/// Provide ways to modify the device tree.
#[derive(PartialEq, Eq)]
pub(crate) enum Overlay<'a> {
    /// Include the DTBO from a path.
    FromPath(&'a Path),
    /// Include a property in /avfnonsecure node. This node is used to specify host provided
    /// properties such as `Id`. pVM firmware does minimal validation of properties in this node.
    NonSecureProp(&'a CStr, &'a [u8]),
}

/// Given a list of `overlays`, return a Device tree containing those!
pub(crate) fn create_overlay<'a>(
    buffer: &'a mut [u8],
    overlays: Vec<Overlay>,
) -> Result<&'a mut Fdt> {
    if overlays.is_empty() {
        return Err(anyhow!("Expected empty list of overlays"));
    }

    let (prop_overlays, path_overlays): (Vec<_>, _) =
        overlays.into_iter().partition(|o| matches!(o, Overlay::NonSecureProp(_, _)));

    let fdt =
        Fdt::create_empty_tree(buffer).map_err(|e| anyhow!("Failed to create empty Fdt: {e:?}"))?;
    let mut root = fdt.root_mut().map_err(|e| anyhow!("Failed to get root: {e:?}"))?;
    let mut node =
        root.add_subnode(cstr!("fragment@0")).map_err(|e| anyhow!("Failed to fragment: {e:?}"))?;
    node.setprop(cstr!("target-path"), b"/\0")
        .map_err(|e| anyhow!("Failed to set target-path: {e:?}"))?;
    let mut node = node
        .add_subnode(cstr!("__overlay__"))
        .map_err(|e| anyhow!("Failed to __overlay__ node: {e:?}"))?;

    if !prop_overlays.is_empty() {
        let mut node = node
            .add_subnode(cstr!("avfnonsecure"))
            .map_err(|e| anyhow!("Failed to ads afvnonsecure node: {e:?}"))?;
        for overlay in prop_overlays {
            if let Overlay::NonSecureProp(name, value) = overlay {
                node.setprop(name, value).map_err(|e| anyhow!("Failed to set property: {e:?}"))?;
            }
        }
    }

    for overlay in path_overlays {
        if let Overlay::FromPath(path) = overlay {
            fdt.append(cstr!("/fragment@0/__overlay__"), path)?;
        }
    }

    Ok(fdt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_overlays_not_allowed() {
        let mut buffer = vec![0_u8; VM_DT_OVERLAY_MAX_SIZE];
        let res = create_overlay(&mut buffer, vec![]);
        assert!(res.is_err());
    }

    #[test]
    fn nonsecure_prop_test() {
        let mut buffer = vec![0_u8; VM_DT_OVERLAY_MAX_SIZE];
        let name = cstr!("XOXO");
        let value_in = b"OXOX";
        let fdt =
            create_overlay(&mut buffer, vec![Overlay::NonSecureProp(name, value_in)]).unwrap();

        let value_dt = fdt
            .node(cstr!("/fragment@0/__overlay__/avfnonsecure"))
            .unwrap()
            .expect("avfnonsecure node doesn't exist")
            .getprop(name)
            .unwrap()
            .expect("Prop not found!");
        assert!(value_dt == value_in);
    }
}
