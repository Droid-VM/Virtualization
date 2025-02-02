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

use android_hardware_security_secretkeeper::aidl::android::hardware::security::secretkeeper::{ISecretkeeper::ISecretkeeper, PublicKey::PublicKey};
use android_system_virtualizationservice::aidl::android::system::virtualizationservice::VirtualMachineConfig::VirtualMachineConfig;
use anyhow::{anyhow, Result};
use binder::Strong;
use crate::aidl::{extract_instance_id, extract_vendor_hashtree_digest, extract_want_updatable, is_secretkeeper_supported, SECRETKEEPER_IDENTIFIER, VM_REFERENCE_DT_ON_HOST_PATH};
use fsfdt::FsFdt;
use libfdt::{Fdt, FdtNodeMut};
use std::io::Read;
use std::ffi::CStr;
use std::fs::{File, read_dir};
use std::path::Path;

pub(crate) const AVF_NODE_NAME: &CStr = c"avf";
pub(crate) const UNTRUSTED_NODE_NAME: &CStr = c"untrusted";
pub(crate) const VM_DT_OVERLAY_PATH: &str = "vm_dt_overlay.dtbo";
pub(crate) const VM_DT_OVERLAY_MAX_SIZE: usize = 2000;

enum OverlayEntry {
    DeferRollbackProtection,
    InstanceId([u8; 64]),
    SecretkeeperPublicKey(Vec<u8>),
    VendorHashtreeDescriptorRootDigest(Vec<u8>),
}

impl OverlayEntry {
    fn key(&self) -> &CStr {
        match self {
            Self::DeferRollbackProtection => c"defer_rollback_protection",
            Self::InstanceId(_) => c"instance_id",
            Self::SecretkeeperPublicKey(_) => c"secretkeeper_public_key",
            Self::VendorHashtreeDescriptorRootDigest(_) => {
                c"vendor_hashtree_descriptor_root_digest"
            }
        }
    }

    fn value(&self) -> &[u8] {
        match self {
            Self::DeferRollbackProtection => &[],
            Self::InstanceId(value) => value,
            Self::SecretkeeperPublicKey(value)
            | Self::VendorHashtreeDescriptorRootDigest(value) => value,
        }
    }

    fn setprop(&self, node: &mut FdtNodeMut) -> Result<()> {
        let key = self.key();
        let value = self.value();
        node.setprop(key, value).map_err(|e| anyhow!("Failed to set {key:?}: {e:?}"))?;
        Ok(())
    }
}

pub(crate) struct DeviceTreeOverlay {
    defer_rollback_protection: Option<OverlayEntry>,
    instance_id: OverlayEntry,
    secretkeeper_public_key: Option<OverlayEntry>,
    vendor_hashtree_descriptor_root_digest: Option<OverlayEntry>,
}

impl DeviceTreeOverlay {
    pub(crate) fn from_config(config: &VirtualMachineConfig) -> Result<Self> {
        let instance_id = extract_instance_id(config);
        let (defer_rollback, sk_pk) = extract_defer_rollback_protection_and_sk_public_key(config)?;
        let vendor_hashtree_descriptor_root_digest = extract_vendor_hashtree_digest(config)?;

        Ok(Self {
            defer_rollback_protection: if defer_rollback {
                Some(OverlayEntry::DeferRollbackProtection)
            } else {
                None
            },
            instance_id: OverlayEntry::InstanceId(instance_id),
            secretkeeper_public_key: sk_pk.map(OverlayEntry::SecretkeeperPublicKey),
            vendor_hashtree_descriptor_root_digest: vendor_hashtree_descriptor_root_digest
                .map(OverlayEntry::VendorHashtreeDescriptorRootDigest),
        })
    }

    pub(crate) fn instance_id(&self) -> [u8; 64] {
        match self.instance_id {
            OverlayEntry::InstanceId(value) => value,
            _ => panic!("wat"),
        }
    }

    pub(crate) fn create_fdt<'a>(&self, buffer: &'a mut [u8]) -> Result<&'a mut Fdt> {
        let fdt = Fdt::create_empty_tree(buffer)
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

        if let Some(sk) = &self.secretkeeper_public_key {
            sk.setprop(&mut avf)?;
        }
        if let Some(ht) = &self.vendor_hashtree_descriptor_root_digest {
            ht.setprop(&mut avf)?;
        }

        let mut untrusted = avf
            .add_subnode(UNTRUSTED_NODE_NAME)
            .map_err(|e| anyhow!("Failed to add untrusted node: {e:?}"))?;

        self.instance_id.setprop(&mut untrusted)?;
        if let Some(defer) = &self.defer_rollback_protection {
            defer.setprop(&mut untrusted)?;
        }

        fdt.pack().map_err(|e| anyhow!("Failed to pack DT overlay, {e:?}"))?;

        Ok(fdt)
    }
}

fn extract_defer_rollback_protection_and_sk_public_key(
    config: &VirtualMachineConfig,
) -> Result<(bool, Option<Vec<u8>>)> {
    let want_updatable = extract_want_updatable(config);
    if want_updatable && is_secretkeeper_supported() {
        let sk: Strong<dyn ISecretkeeper> = binder::wait_for_interface(SECRETKEEPER_IDENTIFIER)?;
        return if sk.getInterfaceVersion()? >= 2 {
            let PublicKey { keyMaterial } = sk.getSecretkeeperIdentity()?;
            Ok((true, Some(keyMaterial)))
        } else {
            Ok((true, extract_sk_public_key_from_host_ref_dt()?))
        };
    }

    Ok((false, extract_sk_public_key_from_host_ref_dt()?))
}

fn extract_sk_public_key_from_host_ref_dt() -> Result<Option<Vec<u8>>> {
    let host_ref_dt = Path::new(VM_REFERENCE_DT_ON_HOST_PATH);
    if !host_ref_dt.exists() || read_dir(host_ref_dt)?.next().is_none() {
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
