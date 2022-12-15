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

use cap_bindgen::{cap_clear_flag, cap_flag_t, cap_free, cap_get_proc, cap_set_proc};
use nix::errno::errno;

/// Possible capabilities flags. This is essentially a redefinition of the cap_flag_t from the
/// libcap (sys/capability.h).
pub type CapFlag = cap_flag_t;

/// Removes all capabilities set for the given flag for this process.
/// See: https://man7.org/linux/man-pages/man7/capabilities.7.html
pub fn drop_caps(flag: CapFlag) -> Result<()> {
    unsafe {
        // SAFETY: we do not manipulate memory handled by libcap.
        let caps = cap_get_proc();
        scopeguard::defer! {
            cap_free(caps as *mut std::os::raw::c_void);
        }
        if cap_clear_flag(caps, flag) < 0 {
            let e = errno();
            bail!("cap_clear_flag failed: {}", e)
        }
        if cap_set_proc(caps) < 0 {
            let e = errno();
            bail!("cap_set_proc failed: {}", e)
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
        drop_caps(CapFlag::CAP_EFFECTIVE).unwrap();
        drop_caps(CapFlag::CAP_PERMITTED).unwrap();
        drop_caps(CapFlag::CAP_INHERITABLE).unwrap()
    }
}
