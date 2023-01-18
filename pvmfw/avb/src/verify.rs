// Copyright 2022, The Android Open Source Project
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

//! This module handles the pvmfw payload verification.

use crate::descriptor::HashDescriptors;
use crate::error::AvbSlotVerifyError;
use crate::ops::{Ops, Payload};
use crate::partition::PartitionName;
use avb_bindgen::AvbVBMetaData;
use core::ffi::c_char;

/// This enum corresponds to the `DebugLevel` in `VirtualMachineConfig`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugLevel {
    /// Not debuggable at all.
    None,
    /// Fully debuggable.
    Full,
}

fn verify_vbmeta_is_from_kernel_partition(
    vbmeta_image: &AvbVBMetaData,
) -> Result<(), AvbSlotVerifyError> {
    match (vbmeta_image.partition_name as *const c_char).try_into() {
        Ok(PartitionName::Kernel) => Ok(()),
        _ => Err(AvbSlotVerifyError::InvalidMetadata),
    }
}

fn verify_vbmeta_only_has_kernel_hash_descriptor(
    hash_descriptors: &HashDescriptors,
) -> Result<(), AvbSlotVerifyError> {
    if hash_descriptors.len() == 1 {
        Ok(())
    } else {
        Err(AvbSlotVerifyError::InvalidMetadata)
    }
}

/// Verifies the payload (signed kernel + initrd) against the trusted public key.
pub fn verify_payload(
    kernel: &[u8],
    initrd: Option<&[u8]>,
    trusted_public_key: &[u8],
) -> Result<DebugLevel, AvbSlotVerifyError> {
    let mut payload = Payload::new(kernel, initrd, trusted_public_key);
    let mut ops = Ops::from(&mut payload);
    let kernel_verify_result = ops.verify_partition(PartitionName::Kernel.as_cstr())?;
    let vbmeta_images = kernel_verify_result.vbmeta_images()?;
    if vbmeta_images.len() != 1 {
        // There can only be one VBMeta.
        return Err(AvbSlotVerifyError::InvalidMetadata);
    }
    let vbmeta_image = vbmeta_images[0];
    verify_vbmeta_is_from_kernel_partition(&vbmeta_image)?;
    // SAFETY: It is safe because the `vbmeta_image` is collected from `AvbSlotVerifyData`,
    // which is returned by `avb_slot_verify()` when the verification succeeds. It is
    // guaranteed by libavb to be non-null and to point to a valid VBMeta structure.
    let hash_descriptors = unsafe { HashDescriptors::new_from(vbmeta_image)? };
    // TODO(b/265897559): Pass the digest in kernel descriptor to DICE.
    let _kernel_descriptor = hash_descriptors.find(PartitionName::Kernel)?;
    if initrd.is_none() {
        verify_vbmeta_only_has_kernel_hash_descriptor(&hash_descriptors)?;
        return Ok(DebugLevel::None);
    }

    let debug_level = if ops.verify_partition(PartitionName::InitrdNormal.as_cstr()).is_ok() {
        DebugLevel::None
    } else if ops.verify_partition(PartitionName::InitrdDebug.as_cstr()).is_ok() {
        DebugLevel::Full
    } else {
        return Err(AvbSlotVerifyError::Verification);
    };
    Ok(debug_level)
}
