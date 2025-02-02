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
use libfdt::Fdt;
use log::warn;
use std::ffi::CStr;
use std::fs::{read_dir, write, File};
use std::io::Read;
use std::path::Path;

const AVF_NODE_NAME: &CStr = c"avf";
const UNTRUSTED_NODE_NAME: &CStr = c"untrusted";
const VM_DT_OVERLAY_MAX_SIZE: usize = 2000;
const VM_DT_OVERLAY_PATH: &str = "vm_dt_overlay.dtbo";
const VM_REFERENCE_DT_ON_HOST_PATH: &str = "/proc/device-tree/avf/reference";

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
        temporary_directory: &Path,
    ) -> Result<Option<File>> {
        if !self.untrusted.defer_rollback_protection
            && self.untrusted.instance_id.is_none()
            && self.trusted.secretkeeper_public_key.is_none()
            && self.trusted.vendor_hashtree_descriptor_root_digest.is_none()
        {
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

        if let Some(secretkeeper_public_key) = &self.trusted.secretkeeper_public_key {
            avf.setprop(c"secretkeeper_public_key", secretkeeper_public_key)?;
        }
        if let Some(vendor_hashtree_descriptor_root_digest) =
            &self.trusted.vendor_hashtree_descriptor_root_digest
        {
            avf.setprop(
                c"vendor_hashtree_descriptor_root_digest",
                vendor_hashtree_descriptor_root_digest,
            )?;
        }

        match (self.untrusted.defer_rollback_protection, self.untrusted.instance_id) {
            (false, None) => {}
            (defer_rollback_protection, instance_id) => {
                let mut untrusted = avf
                    .add_subnode(UNTRUSTED_NODE_NAME)
                    .map_err(|e| anyhow!("Failed to add untrusted node: {e:?}"))?;

                if let Some(instance_id) = instance_id {
                    untrusted.setprop(c"instance_id", &instance_id)?;
                }
                if defer_rollback_protection {
                    untrusted.setprop(c"defer_rollback_protection", &[])?;
                }
            }
        }

        fdt.pack().map_err(|e| anyhow!("Failed to pack DT overlay, {e:?}"))?;

        let dt_output = temporary_directory.join(VM_DT_OVERLAY_PATH);
        write(&dt_output, fdt.as_slice())?;
        Ok(Some(File::open(dt_output)?))
    }
}

pub(crate) fn extract_sk_public_key_from_host_ref_dt() -> Result<Option<Vec<u8>>> {
    let host_ref_dt = Path::new(VM_REFERENCE_DT_ON_HOST_PATH);
    if !host_ref_dt.exists() || read_dir(host_ref_dt)?.next().is_none() {
        warn!("VM reference DT doesn't exist in host DT");
        return Ok(None);
    }

    let mut sk_pk_file = File::open(host_ref_dt.join("avf").join("secretkeeper_public_key"))
        .map_err(|e| anyhow!("Failed to locate secretkeeper_public_key in path: {e:?}"))?;
    let mut key_material: Vec<u8> = vec![];
    sk_pk_file
        .read_to_end(&mut key_material)
        .map_err(|e| anyhow!("Failed to read secretkeepr_public_key in host path: {e:?}"))?;

    Ok(Some(key_material))
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
