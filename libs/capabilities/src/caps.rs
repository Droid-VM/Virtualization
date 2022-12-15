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

//! A rust library wrapping the libcap functionality.

use anyhow::{bail, Result};
use libc::c_int;
use libc::c_void;

#[allow(non_camel_case_types)]
type cap_t = *mut c_void;

/// Capability sets.
/// This is copied from the sys/capability.h
/// See: https://man7.org/linux/man-pages/man7/capabilities.7.html
#[repr(C)]
pub enum CapFlag {
    /// CAP_EFFECTIVE in sys/capability.h
    CapEffective = 0,
    /// CAP_PERMITTED in sys/capability.h
    CapPermitted = 1,
    /// CAP_INHERITABLE in sys/capability.h
    CapInheritable = 2,
}

#[link(name = "cap")]
extern "C" {
    fn cap_get_proc() -> cap_t;
    fn cap_free(ptr: *mut c_void) -> c_int;
    fn cap_set_proc(cap: cap_t) -> c_int;
    fn cap_clear_flag(cap: cap_t, flag: CapFlag) -> c_int;
}

/// Removes all capabilities set for the given flag for this process.
/// See: https://man7.org/linux/man-pages/man7/capabilities.7.html
pub fn drop_caps(flag: CapFlag) -> Result<()> {
    unsafe {
        let caps = cap_get_proc();
        scopeguard::defer! {
            cap_free(caps);
        }
        if cap_clear_flag(caps, flag) < 0 {
            // TODO(ioffe): propagate errno
            bail!("cap_clear_flag failed")
        }
        if cap_set_proc(caps) < 0 {
            // TODO(ioffe): propagate errno
            bail!("cap_set_proc failed")
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Basic test to verify that calling drop_caps doesn't fail
    #[test]
    fn drop_permitted_caps() {
        drop_caps(CapFlag::CapEffective).unwrap();
        drop_caps(CapFlag::CapPermitted).unwrap();
        drop_caps(CapFlag::CapInheritable).unwrap()
    }
}
