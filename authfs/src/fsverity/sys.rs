/*
 * Copyright (C) 2021 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#![allow(non_camel_case_types)]

use std::os::raw::{c_uchar, c_ushort};

pub use authfs_fsverity_bindgen::{fsverity_descriptor, FS_VERITY_HASH_ALG_SHA256};

type __u8 = c_uchar;
type __le16 = c_ushort;

/// An Rust-friendly alternative of fsverity_formatted_digest. The original digest field is a
/// 0-sized array, with the size specified by digest_size.
pub struct fsverity_formatted_digest_sha256 {
    pub magic: [__u8; 8],
    pub digest_algorithm: __le16,
    pub digest_size: __le16,
    pub digest: [__u8; 32], // for sha256
}
