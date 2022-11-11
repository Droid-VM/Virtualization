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

//! Collection of utilities used in virtualizationservice

use std::ffi::CStr;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

/// Returns the name of the given UID in the system's user database.
/// Returns it if one is found, otherwise returns `None`.
/// Copied from the `users` crate and then modified.
pub fn get_name_by_uid(uid: libc::uid_t) -> Option<OsString> {
    let mut passwd = unsafe { std::mem::zeroed::<libc::passwd>() };
    let mut buf = vec![0; 2048];
    let mut result = std::ptr::null_mut::<libc::passwd>();

    loop {
        let r =
            unsafe { libc::getpwuid_r(uid, &mut passwd, buf.as_mut_ptr(), buf.len(), &mut result) };

        if r != libc::ERANGE {
            break;
        }

        let newsize = buf.len().checked_mul(2)?;
        buf.resize(newsize, 0);
    }

    if result.is_null() {
        // There is no such user, or an error has occurred.
        // errno gets set if there’s an error.
        return None;
    }

    if result != &mut passwd {
        // The result of getpwuid_r should be its input passwd.
        return None;
    }

    // SAFETY: the c-string which result.pw_name points to is copied to OsString which is okay
    // to outlive
    unsafe {
        let result = result.read();
        let name = OsStr::from_bytes(CStr::from_ptr(result.pw_name).to_bytes());
        Some(name.to_os_string())
    }
}
