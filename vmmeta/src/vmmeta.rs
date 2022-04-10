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

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct VmMeta {
    header: Header,
    descriptors: Vec<Descriptor>,
}

impl Default for VmMeta {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> VmMeta {
    pub fn new() -> Self {
        VmMeta { header: Header::new(), descriptors: Vec::new() }
    }

    pub fn descriptors(&self) -> &Vec<Descriptor> {
        &self.descriptors
    }

    pub fn add_descriptor(&mut self, descriptor: Descriptor) {
        self.descriptors.push(descriptor)
    }
}

static VMMETA_MAGIC: u32 = 0x54465641;
static VMMETA_VERSION_MAJOR: u16 = 1;
static VMMETA_VERSION_MINOR: u16 = 0;

#[derive(Serialize, Deserialize)]
pub struct Header {
    magic: u32,
    version_major: u16,
    version_minor: u16,
}

impl Default for Header {
    fn default() -> Self {
        Self::new()
    }
}

impl Header {
    pub fn new() -> Header {
        Header {
            magic: VMMETA_MAGIC,
            version_major: VMMETA_VERSION_MAJOR,
            version_minor: VMMETA_VERSION_MINOR,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum Descriptor {
    VbMeta(VbMetaDescriptor),
    Apk(ApkDescriptor),
    Apex(ApexDescriptor),
}

#[derive(Serialize, Deserialize)]
pub struct ResourceIdentifier {
    pub namespace: String ,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct VbMetaDescriptor {
    /// The identifier of this resource.
    #[serde(flatten)]
    pub resource: ResourceIdentifier,
    /// The SHA-512 [VBMeta digest][] calculated from the top-level VBMeta image.
    ///
    /// [VBMeta digest]: https://android.googlesource.com/platform/external/avb/+/master/README.md#the-vbmeta-digest
    pub vbmeta_digest: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct ApkDescriptor {
    /// The identifier of this resource.
    #[serde(flatten)]
    pub resource: ResourceIdentifier,
    //// Digest of the APK's v3 signing block.
    pub signing_block_digest: Vec<u8>,
}

// TODO: need more details
#[derive(Serialize, Deserialize)]
pub struct ApexDescriptor {
    /// The identifier of this resource.
    #[serde(flatten)]
    pub resource: ResourceIdentifier,
    /// The root digest of the APEX payload.
    pub root_digest: Vec<u8>,
}
