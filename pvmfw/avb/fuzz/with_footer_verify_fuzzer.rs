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

#![allow(missing_docs)]
#![no_main]

use avb_bindgen::{AvbFooter, AvbVBMetaImageHeader, AVB_FOOTER_MAGIC};
use libfuzzer_sys::fuzz_target;
use pvmfw_avb::verify_payload;
use std::mem::{size_of, transmute};

fuzz_target!(|kernel: &[u8]| {
    // This fuzzer is mostly supposed to catch the memory corruption in
    // VBMeta parsing. It is unlikely that the randomly generated
    // kernel can pass the kernel verification, so the value of `initrd`
    // is not so important as we won't reach initrd verification with
    // this fuzzer.
    let vbmeta_size = size_of::<AvbVBMetaImageHeader>() + 8;
    if kernel.len() < vbmeta_size {
        return;
    }
    let vbmeta_offset = kernel.len() + vbmeta_size;
    let avb_footer = AvbFooter {
        magic: AVB_FOOTER_MAGIC[0..4].try_into().unwrap(),
        version_major: 1,
        version_minor: 0,
        original_image_size: kernel.len() as u64,
        vbmeta_offset: vbmeta_offset as u64,
        vbmeta_size: vbmeta_size as u64,
        reserved: [0u8; 28],
    };

    let mut modified_kernel = vec![0u8; vbmeta_offset + size_of::<AvbFooter>()];
    modified_kernel[..kernel.len()].copy_from_slice(kernel);
    modified_kernel[kernel.len()..vbmeta_offset].copy_from_slice(&kernel[..vbmeta_size]);
    // SAFETY: It is safe as avb_footer is a valid AvbFooter struct.
    let avb_footer = unsafe { transmute::<AvbFooter, [u8; size_of::<AvbFooter>()]>(avb_footer) };
    modified_kernel[vbmeta_offset..].copy_from_slice(&avb_footer);
    let _ = verify_payload(&modified_kernel, /*initrd=*/ None, &[0u8; 64]);
});
