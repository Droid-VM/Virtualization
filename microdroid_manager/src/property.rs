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

//! Helpers for using system properties.

use anyhow::{bail, Result};
use libc::__system_property_set;
use std::ffi::CString;

/// Set a property to a string value.
pub fn set_string(name: &str, value: &str) -> Result<()> {
    let name = CString::new(name)?;
    let value = CString::new(value)?;
    if unsafe { __system_property_set(name.as_ptr(), value.as_ptr()) } != 0 {
        bail!("failed to set property");
    }
    Ok(())
}
