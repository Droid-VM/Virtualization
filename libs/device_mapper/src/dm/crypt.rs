/*
 * Copyright (C) 2022 The Android Open Source Project
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

// `dm::crypt` module implements the "crypt" target in the device mapper framework. Specifically,
// it provides `DmCryptTargetBuilder` struct which is used to construct a `DmCryptTarget` struct
// which is then given to `DeviceMapper` to create a mapper device.
#![allow(unused_imports)]
#![allow(missing_docs)]
#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use data_model::DataInit;
use std::io::Write;
use std::mem::size_of;
use std::path::Path;

use super::DmTargetSpec;
use crate::util::*;

// The UAPI for the verity target is here.
// https://www.kernel.org/doc/Documentation/device-mapper/verity.txt

// TODO(b/238179332) The demo uses "aes-cbc-plain64". Vold has options of AES256XTS & adiantum.
// What ciphers do we need to support?
pub enum CryptoType {
    AES256XTS,
    // adiantum,
}

///Todo: None of these are properly supported, I hard coded them in the ioctls.
pub struct DmCryptTargetBuilder<'a> {
    cipher: CryptoType,
    key: Option<&'a [u8]>,
    iv_offset: u64,
    device_path: Option<&'a Path>,
    offset: u64,
    device_size: u64,
    // TODO Extend this to include opt_params
}

pub struct DmCryptTarget(Box<[u8]>);

impl DmCryptTarget {
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl<'a> Default for DmCryptTargetBuilder<'a> {
    fn default() -> Self {
        DmCryptTargetBuilder {
            cipher: CryptoType::AES256XTS,
            key: None,
            iv_offset: 0,
            device_path: None,
            offset: 0,
            device_size: 0,
        }
    }
}

impl<'a> DmCryptTargetBuilder<'a> {
    /// Sets the device that will be used as the data device (i.e. providing actual data).
    pub fn data_device(&mut self, p: &'a Path, size: u64) -> &mut Self {
        self.device_path = Some(p);
        self.device_size = size;
        self
    }

    /// Constructs a `DmCryptTarget`.
    pub fn build(&self) -> Result<DmCryptTarget> {
        // The `DmCryptTarget` struct actually is a flattened data consisting of a header and
        // body. The format of the header is `dm_target_spec` as defined in
        // include/uapi/linux/dm-ioctl.h. The format of the body, in case of `verity` target is
        // https://www.kernel.org/doc/html/latest/admin-guide/device-mapper/dm-crypt.html

        let device_path = self
            .device_path
            .context("data device is not set")?
            .to_str()
            .context("data device path is not encoded in utf8")?;

        let cipher = match self.cipher {
            CryptoType::AES256XTS => "aes-cbc-plain64",
            // CryptoType::adiantum => "aes-xts-plain64",
        };

        // Step2: serialize the information according to the spec, which is ...
        // DmTargetSpec{...}
        // <cipher> <key> <iv_offset> <device path> \
        // <offset> [<#opt_params> <opt_params>]

        // TODO: support the optional parameters... if needed.
        let mut body = String::new();
        use std::fmt::Write;
        write!(&mut body, "{} ", cipher)?;
        write!(&mut body, "babebabebabebabebabebabebabebabebabebabebabebabebabebabebabebabe ")?;
        write!(&mut body, "{} ", self.iv_offset)?;
        write!(&mut body, "{} ", device_path)?;
        write!(&mut body, "{} ", self.offset)?;
        write!(&mut body, "\0")?; // null terminator
        println!("params are : {:?}", body);

        let size = size_of::<DmTargetSpec>() + body.len();
        let aligned_size = (size + 7) & !7; // align to 8 byte boundaries
        let padding = aligned_size - size;

        let mut header = DmTargetSpec::new("crypt")?;
        header.sector_start = 0;
        header.length = self.device_size / 512; // number of 512-byte sectors
        header.next = aligned_size as u32;

        let mut buf = Vec::with_capacity(aligned_size);
        buf.write_all(header.as_slice())?;
        buf.write_all(body.as_bytes())?;
        buf.write_all(vec![0; padding].as_slice())?;
        Ok(DmCryptTarget(buf.into_boxed_slice()))
    }
}
