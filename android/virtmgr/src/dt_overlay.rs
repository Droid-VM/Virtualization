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
use std::ffi::CStr;
use std::fs::{write, File};
use std::path::PathBuf;

const AVF_NODE_NAME: &CStr = c"avf";
const UNTRUSTED_NODE_NAME: &CStr = c"untrusted";
const VM_DT_OVERLAY_MAX_SIZE: usize = 2000;
pub(crate) const VM_DT_OVERLAY_PATH: &str = "vm_dt_overlay.dtbo";

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
    /// Create a Device tree overlay containing the provided proc style device tree & properties!
    /// # Arguments
    /// * `dt_output` - The path to the output dtbo that will be populated.
    ///
    /// Example: with true/Some for all `ExtraDeviceTreeProperties` fields:
    /// ```
    ///   {
    ///     fragment@0 {
    ///         target-path = "/";
    ///         __overlay__ {
    ///             avf {
    ///                 secretkeeper-public-key = [ 0xcc 0xdd .. ]
    ///                 vendor-hashtree-descriptor-root-digest = [ 0xaa 0xbb .. ]
    ///                 untrusted {
    ///                     instance-id = [ 0x01 0x23 .. ]
    ///                     defer-rollback-protection = []
    ///                 };
    ///             };
    ///         };
    ///     };
    /// };
    /// ```
    pub(crate) fn maybe_create_device_tree_overlay(
        &self,
        dt_output: &PathBuf,
    ) -> Result<Option<File>> {
        let mut buffer = [0_u8; VM_DT_OVERLAY_MAX_SIZE];
        let fdt = self.maybe_create_device_tree_overlay_impl(&mut buffer)?;

        if let Some(fdt) = fdt {
            write(dt_output, fdt.as_slice())?;
            return Ok(Some(File::open(dt_output)?));
        }

        Ok(None)
    }

    fn maybe_create_device_tree_overlay_impl<'a>(
        &self,
        buffer: &'a mut [u8],
    ) -> Result<Option<&'a mut Fdt>> {
        let mut avf_props = Vec::new();
        if let Some(secretkeeper_public_key) = &self.trusted.secretkeeper_public_key {
            avf_props.push((c"secretkeeper-public-key", secretkeeper_public_key));
        }
        if let Some(vendor_hashtree_descriptor_root_digest) =
            &self.trusted.vendor_hashtree_descriptor_root_digest
        {
            avf_props.push((
                c"vendor-hashtree-descriptor-root-digest",
                vendor_hashtree_descriptor_root_digest,
            ));
        }

        let mut untrusted_props: Vec<(&CStr, &[u8])> = Vec::new();
        let instance_id_prop; // satisfy the borrow checker
        if let Some(instance_id) = self.untrusted.instance_id {
            instance_id_prop = instance_id;
            untrusted_props.push((c"instance-id", &instance_id_prop));
        }
        if self.untrusted.defer_rollback_protection {
            untrusted_props.push((c"defer-rollback-protection", &[]));
        }

        if avf_props.is_empty() && untrusted_props.is_empty() {
            return Ok(None);
        }

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

        Ok(Some(fdt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_props_falsy_returns_none() {
        let extra_props = ExtraDeviceTreeProperties {
            trusted: TrustedDeviceTreeProperties {
                secretkeeper_public_key: None,
                vendor_hashtree_descriptor_root_digest: None,
            },
            untrusted: UntrustedDeviceTreeProperties {
                defer_rollback_protection: false,
                instance_id: None,
            },
        };
        let buf = PathBuf::new();

        assert!(extra_props.maybe_create_device_tree_overlay(&buf).unwrap().is_none())
    }

    #[test]
    fn untrusted_prop_test() {
        let instance_id: [u8; 64] = [1; 64];
        let extra_props = ExtraDeviceTreeProperties {
            trusted: TrustedDeviceTreeProperties {
                secretkeeper_public_key: None,
                vendor_hashtree_descriptor_root_digest: None,
            },
            untrusted: UntrustedDeviceTreeProperties {
                defer_rollback_protection: true,
                instance_id: Some(instance_id),
            },
        };

        let mut buffer = [0_u8; VM_DT_OVERLAY_MAX_SIZE];
        let fdt = extra_props.maybe_create_device_tree_overlay_impl(&mut buffer).unwrap().unwrap();

        let untrusted_node = fdt
            .node(c"/fragment@0/__overlay__/avf/untrusted")
            .unwrap()
            .expect("/avf/untrusted node doesn't exist");

        let instance_id_prop =
            untrusted_node.getprop(c"instance-id").unwrap().expect("instance-id not found!");

        assert_eq!(instance_id_prop, instance_id, "instance ID prop incorrect");

        let defer_rollback_protection: &[u8] = untrusted_node
            .getprop(c"defer-rollback-protection")
            .unwrap()
            .expect("defer-rollback-protection not found!");

        assert_eq!(
            defer_rollback_protection,
            &[] as &[u8],
            "defer rollback protection prop incorrect"
        )
    }

    #[test]
    fn trusted_prop_test() {
        let sk_pk: [u8; 64] = [1; 64];
        let digest: [u8; 64] = [2; 64];
        let extra_props = ExtraDeviceTreeProperties {
            trusted: TrustedDeviceTreeProperties {
                secretkeeper_public_key: Some(sk_pk.to_vec()),
                vendor_hashtree_descriptor_root_digest: Some(digest.to_vec()),
            },
            untrusted: UntrustedDeviceTreeProperties {
                defer_rollback_protection: false,
                instance_id: None,
            },
        };

        let mut buffer = [0_u8; VM_DT_OVERLAY_MAX_SIZE];
        let fdt = extra_props.maybe_create_device_tree_overlay_impl(&mut buffer).unwrap().unwrap();

        let avf_node =
            fdt.node(c"/fragment@0/__overlay__/avf").unwrap().expect("/avf node doesn't exist");

        let sk_pk_prop = avf_node
            .getprop(c"secretkeeper-public-key")
            .unwrap()
            .expect("secretkeeper-public-key not found!");

        assert_eq!(sk_pk, sk_pk_prop, "secretkeeper-public-key prop incorrect");

        let digest_prop: &[u8] = avf_node
            .getprop(c"vendor-hashtree-descriptor-root-digest")
            .unwrap()
            .expect("vendor-hashtree-descriptor-root-digest not found!");

        assert_eq!(digest, digest_prop, "defer rollback protection prop incorrect");

        let untrusted_node = fdt.node(c"/fragment@0/__overlay__/avf/untrusted").unwrap();

        assert!(untrusted_node.is_none());
    }
}
