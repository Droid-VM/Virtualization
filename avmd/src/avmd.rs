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

use apexutil::to_hex_string;
use serde::{Deserialize, Serialize};
use std::fmt;

/// An Avmd struct contains
/// - A header with version information that allows rollback when needed.
/// - A list of descriptors that describe different images.
#[derive(Serialize, Deserialize, Debug)]
pub struct Avmd {
    header: Header,
    descriptors: Vec<Descriptor>,
}

impl fmt::Display for Avmd {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let descriptors = self
            .descriptors
            .iter()
            .map(|descriptor| match descriptor {
                Descriptor::VbMeta(data) => data.to_string(),
                Descriptor::Apk(data) => data.to_string(),
            })
            .collect::<Vec<String>>()
            .join("\n");
        write!(f, "Descriptors:\n{}", descriptors)
    }
}

impl Avmd {
    /// Creates an instance of Avmd with a given list of descriptors.
    pub fn new(descriptors: Vec<Descriptor>) -> Avmd {
        Avmd { header: Header::default(), descriptors }
    }
}

static AVMD_MAGIC: u32 = 0x444d5641;
static AVMD_VERSION_MAJOR: u16 = 1;
static AVMD_VERSION_MINOR: u16 = 0;

/// Header information for AVMD.
#[derive(Serialize, Deserialize, Debug)]
struct Header {
    magic: u32,
    version_major: u16,
    version_minor: u16,
}

impl Default for Header {
    fn default() -> Self {
        Header {
            magic: AVMD_MAGIC,
            version_major: AVMD_VERSION_MAJOR,
            version_minor: AVMD_VERSION_MINOR,
        }
    }
}

/// AVMD descriptor.
#[derive(Serialize, Deserialize, Debug)]
pub enum Descriptor {
    /// Descriptor type for the VBMeta images.
    VbMeta(VbMetaDescriptor),
    /// Descriptor type for APK.
    Apk(ApkDescriptor),
}

/// VbMeta descriptor.
#[derive(Serialize, Deserialize, Debug)]
pub struct VbMetaDescriptor {
    /// The identifier of this resource.
    #[serde(flatten)]
    pub resource: ResourceIdentifier,
    /// The SHA-512 [VBMeta digest][] calculated from the top-level VBMeta image.
    ///
    /// [VBMeta digest]: https://android.googlesource.com/platform/external/avb/+/master/README.md#the-vbmeta-digest
    pub vbmeta_digest: Vec<u8>,
}

impl fmt::Display for VbMetaDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let result = format!(
            "  VBMeta descriptor:
    namespace:             {}
    name:                  {}
    vbmeta digest:         {}",
            self.resource.namespace,
            self.resource.name,
            to_hex_string(&self.vbmeta_digest)
        );
        f.write_str(&result)
    }
}

/// APK descriptor.
#[derive(Serialize, Deserialize, Debug)]
pub struct ApkDescriptor {
    /// The identifier of this resource.
    #[serde(flatten)]
    pub resource: ResourceIdentifier,
    /// The ID of the algoithm used to sign the APK.
    pub signature_algorithm_id: u32,
    /// Digest of the APK's v3 signing block. TODO: fix
    pub apk_digest: Vec<u8>,
}

impl fmt::Display for ApkDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let result = format!(
            "  APK descriptor:
    namespace:             {}
    name:                  {}
    Signing algorithm ID:  {:#x}
    APK digest:            {}",
            self.resource.namespace,
            self.resource.name,
            self.signature_algorithm_id,
            to_hex_string(&self.apk_digest)
        );
        f.write_str(&result)
    }
}

/// Resource identifiers are UUIDv5 values and are intended to
/// represent logical resources.
#[derive(Serialize, Deserialize, Debug)]
pub struct ResourceIdentifier {
    /// Namespace of the resource.
    namespace: String,
    /// Name of the resource.
    name: String,
}

impl ResourceIdentifier {
    /// Creates an instance of ResourceIdentifier with the given
    /// namespace and name.
    pub fn new(namespace: &str, name: &str) -> ResourceIdentifier {
        ResourceIdentifier { namespace: namespace.to_string(), name: name.to_string() }
    }
}
