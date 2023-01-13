// Copyright 2023, The Android Open Source Project
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

//! Support for the debug policy overlay in pvmfw

use alloc::vec;
use core::ffi::CStr;
use log::debug;
use log::error;
use log::info;

#[derive(Debug, Clone)]
pub enum DebugPolicyError {
    /// An unexpected internal error happened.
    InternalError,
    /// The provided FDT was invalid or malformed.
    InvalidFdt,
    /// The provided debug policy FDT was invalid or malformed.
    InvalidDebugPolicyFdt,
}

/// Applies the debug policy device tree overlay to the pVM DT.
///
/// # Safety
///
/// When an error is returned by this function, the input `Fdt` should be
/// discarded as it may have have been partially corrupted during the overlay
/// application process.
unsafe fn apply_debug_policy(
    fdt: &mut libfdt::Fdt,
    debug_policy: &mut [u8],
) -> Result<(), DebugPolicyError> {
    let overlay = libfdt::Fdt::from_mut_slice(debug_policy).map_err(|e| {
        error!("Failed to load the debug policy overlay: {e}");
        DebugPolicyError::InvalidDebugPolicyFdt
    })?;

    fdt.unpack().map_err(|e| {
        error!("Failed to unpack DT for debug policy: {e}");
        DebugPolicyError::InternalError
    })?;

    let fdt = fdt.apply_overlay(overlay).map_err(|e| {
        error!("Failed to apply the debug policy overlay: {e}");
        DebugPolicyError::InvalidDebugPolicyFdt
    })?;

    fdt.pack().map_err(|e| {
        error!("Failed to re-pack DT after debug policy: {e}");
        DebugPolicyError::InternalError
    })
}

/// Dsiables ramdump by removing crashkernel from bootargs in /chosen.
///
/// # Safety
///
/// This may corrupt the input `Fdt` when error happens while editing prop value.
unsafe fn disable_ramdump(fdt: &mut libfdt::Fdt) -> Result<(), DebugPolicyError> {
    let chosen_path = CStr::from_bytes_with_nul_unchecked(b"/chosen\0");
    let bootargs_name = CStr::from_bytes_with_nul_unchecked(b"bootargs\0");

    let chosen = match fdt.node(chosen_path).map_err(|e| {
        error!("Failed to find /chosen: {e}");
        DebugPolicyError::InvalidFdt
    })? {
        Some(node) => node,
        None => {
            debug!("/chosen node doesn't exist. Assumes that rampdump is disabled already");
            return Ok(());
        }
    };

    let bootargs = match chosen.getprop_str(bootargs_name).map_err(|e| {
        error!("Failed to find bootargs prop: {e}");
        DebugPolicyError::InvalidFdt
    })? {
        Some(value) if !value.to_bytes().is_empty() => value,
        Some(_) => {
            debug!("bootargs prop is empty. Assumes that rampdump is disabled already");
            return Ok(());
        }
        None => {
            debug!("bootargs prop doesn't exist. Assumes that rampdump is disabled already");
            return Ok(());
        }
    };

    // TODO: Improve add 'crashkernel=17MB' only when it's unnecessary.
    //       Currently 'crashkernel=17MB' in virtualizationservice and passed by
    //       chosen node, because it's not exactly a debug policy but a
    //       configuration. However, it's actually microdroid specific
    //       so we need a way to generalize it.
    let mut args = vec![];
    for arg in bootargs.to_bytes().split(|byte| byte.is_ascii_whitespace()) {
        if arg.is_empty() || arg.starts_with(b"crashkernel=") {
            continue;
        }
        args.push(arg);
    }
    let mut new_bootargs = args.as_slice().join(&b" "[..]);
    new_bootargs.push(b'\0');

    // We've checked existence of /chosen node at the beginning.
    let mut chosen_mut = fdt.node_mut(chosen_path).unwrap().unwrap();
    chosen_mut.setprop(bootargs_name, new_bootargs.as_slice()).map_err(|e| {
        error!("Failed to remove crashkernel. fdt might have been corrupted: {e}");
        DebugPolicyError::InternalError
    })
}

/// Returns true only if fdt has ramdump prop in the /avf/guest/common node with value <1>
fn is_ramdump_enabled(fdt: &libfdt::Fdt) -> Result<bool, DebugPolicyError> {
    // SAFETY - Safe to call CStr::from_bytes_with_nul_unchecked() because
    //          the strings are nul terminated.
    unsafe {
        let common = match fdt
            .node(CStr::from_bytes_with_nul_unchecked(b"/avf/guest/common\0"))
            .map_err(|e| {
                error!("Failed to find /avf/guest/common node: {e}");
                DebugPolicyError::InvalidDebugPolicyFdt
            })? {
            Some(node) => node,
            None => {
                debug!("/avf/guest/common node doesn't exist. Assumes no ramdump");
                return Ok(false);
            }
        };

        match common.getprop_u32(CStr::from_bytes_with_nul_unchecked(b"ramdump\0")).map_err(
            |e| {
                error!("Failed to find /avf/guest/common node: {e}");
                DebugPolicyError::InvalidDebugPolicyFdt
            },
        )? {
            Some(0) => Ok(false),
            Some(_) => {
                debug!("None <0> value for ramdump. Assumes ramdump");
                Ok(true)
            }
            None => {
                debug!("ramdump isn't specified in debug policy. Assumes no ramdump");
                Ok(false)
            }
        }
    }
}

/// Handles debug policies.
///
/// # Safety
///
/// This may corrupt the input `Fdt` when overlaying debug policy or applying
/// ramdump configuration.
pub unsafe fn handle_debug_policy(
    fdt: &mut libfdt::Fdt,
    debug_policy: Option<&mut [u8]>,
) -> Result<(), DebugPolicyError> {
    if debug_policy.is_some() {
        apply_debug_policy(fdt, debug_policy.unwrap())?;
    } else {
        info!("No debug policy found");
    }

    // Handles ramdump in the debug policy
    if is_ramdump_enabled(fdt)? {
        info!("ramdump is enabled by debug policy.");
        return Ok(());
    }
    disable_ramdump(fdt)
}
