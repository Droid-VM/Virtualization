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
use fsfdt::FsFdt;
use libfdt::Fdt;
use log::warn;
use std::ffi::CStr;
use std::fs::{write, File};
use std::io::Read;
use std::path::{Path, PathBuf};

const AVF_NODE_NAME: &CStr = c"avf";
const UNTRUSTED_NODE_NAME: &CStr = c"untrusted";
const VM_DT_OVERLAY_MAX_SIZE: usize = 2000;
pub(crate) const VM_DT_OVERLAY_PATH: &str = "vm_dt_overlay.dtbo";
const SECRETKEEPER_PUBLIC_KEY_ON_HOST_DT: &str =
    "/proc/device-tree/avf/reference/avf/secretkeeper_public_key";

pub(crate) struct TrustedDeviceTreeProperties {
    pub(crate) secretkeeper_public_key: Option<Vec<u8>>,
    pub(crate) vendor_hashtree_descriptor_root_digest: Option<Vec<u8>>,
}

pub(crate) struct UntrustedDeviceTreeProperties {
    pub(crate) defer_rollback_protection: bool,
    pub(crate) instance_id: Option<[u8; 64]>,
}

pub(crate) struct ExtraDeviceTreeProperties {
    pub(crate) trusted: TrustedDeviceTreeProperties,
    pub(crate) untrusted: UntrustedDeviceTreeProperties,
}

impl ExtraDeviceTreeProperties {
    pub(crate) fn maybe_create_device_tree_overlay(
        &self,
        dt_output: &PathBuf,
    ) -> Result<Option<File>> {
        let mut avf_props = Vec::new();
        if let Some(secretkeeper_public_key) = &self.trusted.secretkeeper_public_key {
            avf_props.push((c"secretkeeper_public_key", secretkeeper_public_key));
        }
        if let Some(vendor_hashtree_descriptor_root_digest) =
            &self.trusted.vendor_hashtree_descriptor_root_digest
        {
            avf_props.push((
                c"vendor_hashtree_descriptor_root_digest",
                vendor_hashtree_descriptor_root_digest,
            ));
        }

        let mut untrusted_props: Vec<(&CStr, &[u8])> = Vec::new();
        let instance_id_prop: [u8; 64]; // satisfy borrow checker
        match (self.untrusted.defer_rollback_protection, self.untrusted.instance_id) {
            (false, None) => {}
            (defer_rollback_protection, instance_id) => {
                if let Some(instance_id) = instance_id {
                    instance_id_prop = instance_id;
                    untrusted_props.push((c"instance_id", &instance_id_prop));
                }
                if defer_rollback_protection {
                    untrusted_props.push((c"defer_rollback_protection", &[]));
                }
            }
        }

        if avf_props.is_empty() && untrusted_props.is_empty() {
            return Ok(None);
        }

        let mut buffer = [0_u8; VM_DT_OVERLAY_MAX_SIZE];
        let fdt = Fdt::create_empty_tree(&mut buffer)
            .map_err(|e| anyhow!("Failed to create empty Fdt: {e:?}"))?;
        let mut fragment = fdt
            .root_mut()
            .add_subnode(c"fragment@0")
            .map_err(|e| anyhow!("Failed to add fragment node: {e:?}"))?;
        fragment
            .setprop(c"target-path", b"/\0")
            .map_err(|e| anyhow!("Failed to set target-path property: {e:?}"))?;
        let overlay = fragment
            .add_subnode(c"__overlay__")
            .map_err(|e| anyhow!("Failed to add __overlay__ node: {e:?}"))?;
        let mut avf = overlay
            .add_subnode(AVF_NODE_NAME)
            .map_err(|e| anyhow!("Failed to add avf node: {e:?}"))?;

        for (key, val) in avf_props {
            avf.setprop(key, val)?;
        }

        if !untrusted_props.is_empty() {
            let mut untrusted = avf
                .add_subnode(UNTRUSTED_NODE_NAME)
                .map_err(|e| anyhow!("Failed to add untrusted node: {e:?}"))?;

            for (key, val) in untrusted_props {
                untrusted.setprop(key, val)?;
            }
        }

        fdt.pack().map_err(|e| anyhow!("Failed to pack DT overlay, {e:?}"))?;
        write(dt_output, fdt.as_slice())?;
        Ok(Some(File::open(dt_output)?))
    }
}

pub(crate) fn extract_sk_public_key_from_host_dt() -> Result<Option<Vec<u8>>> {
    let sk_pk_path = Path::new(SECRETKEEPER_PUBLIC_KEY_ON_HOST_DT);
    let mut sk_pk_file = File::open(sk_pk_path).map_err(|e| {
        warn!("secretkeeper_public_key file not found on host DT");
        anyhow!("Failed to locate secretkeeper_public_key in host DT: {e:?}")
    })?;

    let mut key_material: Vec<u8> = vec![];
    sk_pk_file
        .read_to_end(&mut key_material)
        .map_err(|e| anyhow!("Failed to read secretkeepr_public_key in host path: {e:?}"))?;

    Ok(Some(key_material))
}

/// Create a Device tree overlay containing the provided proc style device tree & properties!
/// # Arguments
/// * `dt_path` - (Optional) Path to (proc style) device tree to be included in the overlay.
/// * `untrusted_props` - Include a property in /avf/untrusted node. This node is used to specify
///   host provided properties such as `instance-id`.
/// * `trusted_props` - Include a property in /avf node. This overwrites nodes included with
///   `dt_path`. In pVM, pvmfw will reject if it doesn't match the value in pvmfw config.
///
/// Example: with `create_device_tree_overlay(_, _, [("instance-id", _),], [("digest", _),])`
/// ```
///   {
///     fragment@0 {
///         target-path = "/";
///         __overlay__ {
///             avf {
///                 digest = [ 0xaa 0xbb .. ]
///                 untrusted { instance-id = [ 0x01 0x23 .. ] }
///               }
///             };
///         };
///     };
/// };
/// ```
#[allow(dead_code)]
pub(crate) fn create_device_tree_overlay<'a>(
    buffer: &'a mut [u8],
    dt_path: Option<&'a Path>,
    untrusted_props: &[(&'a CStr, &'a [u8])],
    trusted_props: &[(&'a CStr, &'a [u8])],
) -> Result<&'a mut Fdt> {
    if dt_path.is_none() && untrusted_props.is_empty() && trusted_props.is_empty() {
        return Err(anyhow!("Expected at least one device tree addition"));
    }

    let fdt =
        Fdt::create_empty_tree(buffer).map_err(|e| anyhow!("Failed to create empty Fdt: {e:?}"))?;
    let mut fragment = fdt
        .root_mut()
        .add_subnode(c"fragment@0")
        .map_err(|e| anyhow!("Failed to add fragment node: {e:?}"))?;
    fragment
        .setprop(c"target-path", b"/\0")
        .map_err(|e| anyhow!("Failed to set target-path property: {e:?}"))?;
    let overlay = fragment
        .add_subnode(c"__overlay__")
        .map_err(|e| anyhow!("Failed to add __overlay__ node: {e:?}"))?;
    let avf =
        overlay.add_subnode(AVF_NODE_NAME).map_err(|e| anyhow!("Failed to add avf node: {e:?}"))?;

    if !untrusted_props.is_empty() {
        let mut untrusted = avf
            .add_subnode(UNTRUSTED_NODE_NAME)
            .map_err(|e| anyhow!("Failed to add untrusted node: {e:?}"))?;
        for (name, value) in untrusted_props {
            untrusted
                .setprop(name, value)
                .map_err(|e| anyhow!("Failed to set untrusted property: {e:?}"))?;
        }
    }

    // Read dt_path from host DT and overlay onto fdt.
    if let Some(path) = dt_path {
        fdt.overlay_onto(c"/fragment@0/__overlay__", path)?;
    }

    if !trusted_props.is_empty() {
        let mut avf = fdt
            .node_mut(c"/fragment@0/__overlay__/avf")
            .map_err(|e| anyhow!("Failed to search avf node: {e:?}"))?
            .ok_or(anyhow!("Failed to get avf node"))?;
        for (name, value) in trusted_props {
            avf.setprop(name, value)
                .map_err(|e| anyhow!("Failed to set trusted property: {e:?}"))?;
        }
    }

    fdt.pack().map_err(|e| anyhow!("Failed to pack DT overlay, {e:?}"))?;

    Ok(fdt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_overlays_not_allowed() {
        let mut buffer = vec![0_u8; VM_DT_OVERLAY_MAX_SIZE];
        let res = create_device_tree_overlay(&mut buffer, None, &[], &[]);
        assert!(res.is_err());
    }

    #[test]
    fn untrusted_prop_test() {
        let mut buffer = vec![0_u8; VM_DT_OVERLAY_MAX_SIZE];
        let prop_name = c"XOXO";
        let prop_val_input = b"OXOX";
        let fdt =
            create_device_tree_overlay(&mut buffer, None, &[(prop_name, prop_val_input)], &[])
                .unwrap();

        let prop_value_dt = fdt
            .node(c"/fragment@0/__overlay__/avf/untrusted")
            .unwrap()
            .expect("/avf/untrusted node doesn't exist")
            .getprop(prop_name)
            .unwrap()
            .expect("Prop not found!");
        assert_eq!(prop_value_dt, prop_val_input, "Unexpected property value");
    }

    #[test]
    fn trusted_prop_test() {
        let mut buffer = vec![0_u8; VM_DT_OVERLAY_MAX_SIZE];
        let prop_name = c"XOXOXO";
        let prop_val_input = b"OXOXOX";
        let fdt =
            create_device_tree_overlay(&mut buffer, None, &[], &[(prop_name, prop_val_input)])
                .unwrap();

        let prop_value_dt = fdt
            .node(c"/fragment@0/__overlay__/avf")
            .unwrap()
            .expect("/avf node doesn't exist")
            .getprop(prop_name)
            .unwrap()
            .expect("Prop not found!");
        assert_eq!(prop_value_dt, prop_val_input, "Unexpected property value");
    }
}
