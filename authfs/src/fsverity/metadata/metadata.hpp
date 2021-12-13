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

#ifndef AUTHFS_FSVERITY_METADATA_H
#define AUTHFS_FSVERITY_METADATA_H

// This file contains the format of FSVerity metadata (.fsv_meta).
// TODO(b/193113326): sync with build/make/tools/releasetools/fsverity_metadata_generator.py

#include <stddef.h>
#include <stdint.h>
#include <linux/fsverity.h>

const uint64_t CHUNK_SIZE = 4096;

enum class FSVERITY_SIGNATURE_TYPE : __le32 {
    NONE = 0,
    PKCS7 = 1,
    RAW = 2,
};

struct fsverity_metadata_header {
    __le32 version;
    fsverity_descriptor descriptor;
    FSVERITY_SIGNATURE_TYPE signature_type;
    __le32 signature_size;
} __attribute__((packed));

#endif   // AUTHFS_FSVERITY_METADATA_H
