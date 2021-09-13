// Copyright 2021, The Android Open Source Project
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

//! Routines for handling APEX payload

#![allow(dead_code)]
use anyhow::Result;
use std::fs::File;
use std::io::Read;
use zip::ZipArchive;

const APEX_PUBKEY_ENTRY: &str = "apex_pubkey";
const APEX_PAYLOAD_ENTRY: &str = "apex_payload.img";

/// Verification result holds public key and root digest of apex_payload.img
pub struct ApexVerificationResult {
    pub public_key: Vec<u8>,
    pub root_digest: Vec<u8>,
}

/// Verify APEX payload by AVB verification and return public key and root digest
pub fn verify(path: &str) -> Result<ApexVerificationResult> {
    let apex_file = File::open(path)?;
    let (public_key, image_offset, image_size) = get_public_key_and_image_info(&apex_file)?;
    let root_digest = avb_rs::verify(apex_file, image_offset, image_size, &public_key)?;
    Ok(ApexVerificationResult { public_key, root_digest })
}

fn get_public_key_and_image_info(apex_file: &File) -> Result<(Vec<u8>, u64, u64)> {
    let mut z = ZipArchive::new(apex_file)?;

    let mut public_key = Vec::new();
    z.by_name(APEX_PUBKEY_ENTRY)?.read_to_end(&mut public_key)?;

    let (image_offset, image_size) =
        z.by_name(APEX_PAYLOAD_ENTRY).map(|f| (f.data_start(), f.size()))?;

    Ok((public_key, image_offset, image_size))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn to_hex_string(buf: &[u8]) -> String {
        buf.iter().map(|b| format!("{:02x}", b)).collect()
    }
    #[test]
    fn test_open_apex() {
        let res = verify("tests/data/test.apex").unwrap();
        assert_eq!(
            to_hex_string(&res.root_digest),
            "fe11ab17da0a3a738b54bdc3a13f6139cbdf91ec32f001f8d4bbbf8938e04e39"
        );
    }
}
