// Copyright (C) 2021 The Android Open Source Project
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

//! This crate provides the libavb wrapper.

use anyhow::{anyhow, ensure, Result};
use avb_bindgen::*;
use std::ffi::{c_void, CStr};
use std::io::{Read, Seek, SeekFrom};
use std::mem::{size_of, zeroed};
use std::ops::Deref;
use std::ptr::null_mut;
use std::slice::{from_raw_parts, from_raw_parts_mut};

// Manual addition of a missing enum
#[allow(non_camel_case_types, dead_code)]
#[repr(u8)]
enum AvbDescriptorTag {
    AVB_DESCRIPTOR_TAG_PROPERTY = 0,
    AVB_DESCRIPTOR_TAG_HASHTREE,
    AVB_DESCRIPTOR_TAG_HASH,
    AVB_DESCRIPTOR_TAG_KERNEL_CMDLINE,
    AVB_DESCRIPTOR_TAG_CHAIN_PARTITION,
}

const FOOTER_SIZE: usize = size_of::<AvbFooter>();
const HASHTREE_DESCRIPTOR_SIZE: usize = size_of::<AvbHashtreeDescriptor>();

/// Verify VBmeta image and return root digest
pub fn verify<R: Read + Seek>(
    image: R,
    offset: u64,
    size: u64,
    public_key: &[u8],
) -> Result<Vec<u8>> {
    let vbmeta = VbMeta::from(image, offset, size)?;
    vbmeta.verify(public_key)?;
    for &descriptor in vbmeta.descriptors()?.iter() {
        if let Ok(hashtree_descriptor) = HashtreeDescriptor::from(descriptor) {
            return hashtree_descriptor.root_digest();
        }
    }
    Err(anyhow!("HashtreeDescriptor is not found."))
}

struct VbMeta {
    data: Vec<u8>,
}

impl VbMeta {
    // Read a VbMeta data from a given image
    fn from<R: Read + Seek>(mut image: R, offset: u64, size: u64) -> Result<VbMeta> {
        // Get AvbFooter first
        image.seek(SeekFrom::Start(offset + size - FOOTER_SIZE as u64))?;
        // SAFETY: AvbDescriptor is a "repr(C,packed)" struct from bindgen
        let mut footer: AvbFooter = unsafe { zeroed() };
        // SAFETY: safe to read because of seek(-FOOTER_SIZE) above
        unsafe {
            let footer_slice = from_raw_parts_mut(&mut footer as *mut _ as *mut u8, FOOTER_SIZE);
            image.read_exact(footer_slice)?;
            ensure!(avb_footer_validate_and_byteswap(&footer, &mut footer));
        }
        // Get VbMeta block
        image.seek(SeekFrom::Start(offset + footer.vbmeta_offset))?;
        let vbmeta_size = footer.vbmeta_size as usize;
        let mut data = vec![0u8; vbmeta_size];
        image.read_exact(&mut data)?;
        Ok(VbMeta { data })
    }
    // Verify VbMeta image. Its enclosed public key should match with a given public key.
    fn verify(&self, outer_public_key: &[u8]) -> Result<()> {
        // SAFETY: self.data points to a valid VBMeta data and avb_vbmeta_image_verify should work fine
        // with it
        let public_key = unsafe {
            let mut pk_ptr: *const u8 = null_mut();
            let mut pk_len: usize = 0;
            let res = avb_vbmeta_image_verify(
                self.data.as_ptr(),
                self.data.len(),
                &mut pk_ptr,
                &mut pk_len,
            );
            ensure!(
                res == AvbVBMetaVerifyResult_AVB_VBMETA_VERIFY_RESULT_OK,
                CStr::from_ptr(avb_vbmeta_verify_result_to_string(res))
                    .to_string_lossy()
                    .into_owned()
            );
            from_raw_parts(pk_ptr, pk_len)
        };

        ensure!(public_key == outer_public_key, "Public key mismatch with a given one.");
        Ok(())
    }
    // Return a slice of AvbDescriptor pointers
    fn descriptors(&self) -> Result<Descriptors> {
        let mut num: usize = 0;
        // SAFETY: ptr will be freed by Descriptor.
        Ok(unsafe {
            let ptr = avb_descriptor_get_all(self.data.as_ptr(), self.data.len(), &mut num);
            ensure!(!ptr.is_null(), "VbMeta has no descriptors.");
            let all = from_raw_parts(ptr, num);
            Descriptors { ptr, all }
        })
    }
}

struct HashtreeDescriptor {
    ptr: *const u8,
    inner: AvbHashtreeDescriptor,
}

impl HashtreeDescriptor {
    fn from(descriptor: *const AvbDescriptor) -> Result<HashtreeDescriptor> {
        // SAFETY: AvbDescriptor is a "repr(C,packed)" struct from bindgen
        let mut desc: AvbDescriptor = unsafe { zeroed() };
        // SAFETY: both points to valid AvbDescriptor pointers
        unsafe {
            ensure!(avb_descriptor_validate_and_byteswap(descriptor, &mut desc));
        }
        ensure!({ desc.tag } == AvbDescriptorTag::AVB_DESCRIPTOR_TAG_HASHTREE as u64);
        // SAFETY: AvbHashtreeDescriptor is a "repr(C, packed)" struct from bindgen
        let mut hashtree_descriptor: AvbHashtreeDescriptor = unsafe { zeroed() };
        // SAFETY: With tag == AVB_DESCRIPTOR_TAG_HASHTREE, descriptor should point to
        // a AvbHashtreeDescriptor.
        unsafe {
            ensure!(avb_hashtree_descriptor_validate_and_byteswap(
                descriptor as *const AvbHashtreeDescriptor,
                &mut hashtree_descriptor,
            ));
        }
        Ok(Self { ptr: descriptor as *const u8, inner: hashtree_descriptor })
    }
    fn root_digest(&self) -> Result<Vec<u8>> {
        // SAFETY: digest_ptr should point to a valid buffer of root_digest_len
        let root_digest = unsafe {
            let digest_ptr = self.ptr.offset(
                HASHTREE_DESCRIPTOR_SIZE as isize
                    + self.inner.partition_name_len as isize
                    + self.inner.salt_len as isize,
            );
            from_raw_parts(digest_ptr, self.inner.root_digest_len as usize)
        };
        Ok(root_digest.to_owned())
    }
}

// Wraps pointer to a heap-allocated array of AvbDescriptor pointers
struct Descriptors<'a> {
    ptr: *mut *const AvbDescriptor,
    all: &'a [*const AvbDescriptor],
}

// Wrapped pointer should be freed with avb_free.
impl Drop for Descriptors<'_> {
    fn drop(&mut self) {
        // SAFETY: ptr is allocated by avb_descriptor_get_all
        unsafe { avb_free(self.ptr as *mut c_void) }
    }
}

impl<'a> Deref for Descriptors<'a> {
    type Target = &'a [*const AvbDescriptor];
    fn deref(&self) -> &Self::Target {
        &self.all
    }
}
