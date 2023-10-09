/*
 * Copyright 2023 The Android Open Source Project
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

//! Handle parsing of APK manifest files.
//! The manifest file is written as XML text, but is stored in the APK
//! as Android binary compressed XML. This library is a wrapper around
//! a thin C++ wrapper around libandroidfw, which contains the same
//! parsing code as used by package manager and aapt2 (amongst other
//! things).

use anyhow::{bail, Context, Result};
use apkmanifest_bindgen::{extractManifestInfo, freeManifestInfo, getPackageName};
use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Information extracted from the Android manifest inside an APK.
pub struct ApkManifestInfo {
    /// The package name of the APK.
    pub package: String,
}

/// Given the bytes of a binary compressed AndroidManifest.xml, extract and
/// return information from it.
pub fn extract_manifest_info(apk_path: &Path) -> Result<ApkManifestInfo> {
    let apk_path = CString::new(apk_path.as_os_str().as_bytes()).context("Invalid APK path")?;

    // Safety: The function only reads the memory range we specify and does not hold
    // any reference to it.
    let native_info = unsafe { extractManifestInfo(apk_path.as_ptr()) };
    scopeguard::defer! {
        // Safety: Accepts any value returned from extractManifestInfo, including null.
        // We must call this exactly once, which we do here.
        unsafe { freeManifestInfo(native_info); }
    }

    if native_info.is_null() {
        bail!("Failed to parse manifest")
    };

    // Safety: It is always valid to call getPackageName with a valid native_info, which we have,
    // and it always returns a valid nul-terminated C string.
    let package = unsafe { CStr::from_ptr(getPackageName(native_info)) };
    let package = package.to_str().context("Invalid package name")?.to_string();
    Ok(ApkManifestInfo { package })
}
