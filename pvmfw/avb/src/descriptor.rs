// Copyright 2023, The Android Open Source Project
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

//! Structs and functions relating to the descriptors.

#![warn(unsafe_op_in_unsafe_fn)]

use crate::error::{AvbIOError, AvbSlotVerifyError};
use crate::partition::PartitionName;
use crate::utils::{self, is_not_null, to_nonnull, to_usize, usize_checked_add};
use avb_bindgen::{
    avb_descriptor_foreach, avb_hash_descriptor_validate_and_byteswap, AvbDescriptor,
    AvbHashDescriptor, AvbVBMetaData,
};
use core::{
    ffi::c_void,
    iter,
    mem::{size_of, MaybeUninit},
    ops::Range,
    slice::{self, Iter},
};

/// Currently we use SHA256 to hash the kernel and initrd, their digest size
/// is then 256 bits.
const DIGEST_SIZE: usize = 32;

#[derive(Default)]
pub(crate) struct HashDescriptors {
    descriptors: [Option<HashDescriptor>; Self::MAX_NUM_OF_DESCRIPTORS],
    num_descriptors: usize,
}

type HashDescriptorRefIter<'a> =
    iter::Map<Iter<'a, Option<HashDescriptor>>, fn(&Option<HashDescriptor>) -> &HashDescriptor>;

impl HashDescriptors {
    /// The maximum number of descriptors corresponds to the number of known partitions.
    const MAX_NUM_OF_DESCRIPTORS: usize = 3;

    fn push(&mut self, descriptor: HashDescriptor) -> utils::Result<()> {
        if self.num_descriptors >= Self::MAX_NUM_OF_DESCRIPTORS {
            return Err(AvbIOError::Io);
        }
        if self.iter().any(|d| d.partition_name_eq(&descriptor)) {
            return Err(AvbIOError::Io);
        }
        self.descriptors[self.num_descriptors] = Some(descriptor);
        self.num_descriptors += 1;
        Ok(())
    }

    pub(crate) fn len(&self) -> usize {
        self.num_descriptors
    }

    /// Finds the `HashDescriptor` for the given `PartitionName`.
    /// Throws an error if no corresponding descriptor found.
    pub(crate) fn find(
        &self,
        partition_name: PartitionName,
    ) -> Result<&HashDescriptor, AvbSlotVerifyError> {
        self.iter()
            .find(|d| d.partition_name == partition_name)
            .ok_or(AvbSlotVerifyError::InvalidMetadata)
    }

    fn iter(&self) -> HashDescriptorRefIter {
        self.descriptors[..self.num_descriptors].iter().map(|d| d.as_ref().unwrap())
    }
}

unsafe extern "C" fn check_and_save_descriptor(
    descriptor: *const AvbDescriptor,
    user_data: *mut c_void,
) -> bool {
    try_check_and_save_descriptor(descriptor, user_data).is_ok()
}

fn try_check_and_save_descriptor(
    descriptor: *const AvbDescriptor,
    user_data: *mut c_void,
) -> utils::Result<()> {
    let desc = AvbHashDescriptorWrap::try_from(descriptor)?;
    // SAFETY: It is safe because `descriptor` is a nonnull pointer pointing to a valid
    // struct.
    let data = unsafe { slice::from_raw_parts(descriptor as *const u8, desc.data_len()?) };
    let mut descriptors = to_nonnull(user_data as *mut HashDescriptors)?;
    // SAFETY: It is safe because `descriptors` is a nonnull pointer pointing to a valid
    // struct.
    let descriptors = unsafe { descriptors.as_mut() };
    descriptors.push(HashDescriptor::new(&desc, data)?)
}

/// Builds `HashDescriptors` from `AvbVBMetaData`.
/// Throws an error if the given `AvbVBMetaData` contains non hash descriptor, hash
/// descriptor of unknown `PartitionName` or duplicated hash descriptors.
impl TryFrom<AvbVBMetaData> for HashDescriptors {
    type Error = AvbSlotVerifyError;

    fn try_from(vbmeta: AvbVBMetaData) -> Result<Self, Self::Error> {
        is_not_null(vbmeta.vbmeta_data).map_err(|_| AvbSlotVerifyError::Io)?;
        let mut descriptors = HashDescriptors::default();
        // SAFETY: It is safe as the raw pointer `vbmeta.vbmeta_data` is a nonnull pointer.
        if !unsafe {
            avb_descriptor_foreach(
                vbmeta.vbmeta_data,
                vbmeta.vbmeta_size,
                Some(check_and_save_descriptor),
                &mut descriptors as *mut _ as *mut c_void,
            )
        } {
            return Err(AvbSlotVerifyError::InvalidMetadata);
        }
        Ok(descriptors)
    }
}

pub(crate) struct HashDescriptor {
    partition_name: PartitionName,
    /// TODO(b/265897559): Pass this digest to DICE.
    #[allow(dead_code)]
    digest: [u8; DIGEST_SIZE],
}

impl HashDescriptor {
    fn new(desc: &AvbHashDescriptorWrap, data: &[u8]) -> utils::Result<Self> {
        let partition_name = data
            .get(desc.partition_name_range()?)
            .ok_or(AvbIOError::RangeOutsidePartition)?
            .try_into()?;
        let partition_digest =
            data.get(desc.digest_range()?).ok_or(AvbIOError::RangeOutsidePartition)?;
        let mut digest = [0u8; DIGEST_SIZE];
        digest.copy_from_slice(partition_digest);
        Ok(Self { partition_name, digest })
    }

    fn partition_name_eq(&self, other: &HashDescriptor) -> bool {
        self.partition_name == other.partition_name
    }
}

/// `AvbHashDescriptor` contains the metadata for the given descriptor.
struct AvbHashDescriptorWrap(AvbHashDescriptor);

impl TryFrom<*const AvbDescriptor> for AvbHashDescriptorWrap {
    type Error = AvbIOError;

    fn try_from(descriptor: *const AvbDescriptor) -> Result<Self, Self::Error> {
        is_not_null(descriptor)?;
        // SAFETY: It is safe as the raw pointer `descriptor` is a nonnull pointer.
        let desc = unsafe {
            let mut desc = MaybeUninit::uninit();
            if !avb_hash_descriptor_validate_and_byteswap(
                descriptor as *const AvbHashDescriptor,
                desc.as_mut_ptr(),
            ) {
                return Err(AvbIOError::Io);
            }
            desc.assume_init()
        };
        Ok(Self(desc))
    }
}

impl AvbHashDescriptorWrap {
    fn data_len(&self) -> utils::Result<usize> {
        usize_checked_add(
            size_of::<AvbDescriptor>(),
            to_usize(self.0.parent_descriptor.num_bytes_following)?,
        )
    }

    fn partition_name_end(&self) -> utils::Result<usize> {
        usize_checked_add(size_of::<AvbHashDescriptor>(), to_usize(self.0.partition_name_len)?)
    }

    fn partition_name_range(&self) -> utils::Result<Range<usize>> {
        let start = size_of::<AvbHashDescriptor>();
        Ok(start..(self.partition_name_end()?))
    }

    fn digest_range(&self) -> utils::Result<Range<usize>> {
        let start = usize_checked_add(self.partition_name_end()?, to_usize(self.0.salt_len)?)?;
        let end = usize_checked_add(start, to_usize(self.0.digest_len)?)?;
        Ok(start..end)
    }
}
