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

//! Low-level entry and exit points of pvmfw.

use crate::helpers::FDT_MAX_SIZE;
use crate::jump_to_payload;
use crate::mmio_guard;
use core::{fmt, slice};
use log::{debug, LevelFilter};
use vmbase::{console, logger, main, power::reboot};

#[derive(Debug, Clone)]
enum RebootReason {
    /// An unexpected internal error happened.
    InternalError,
}

impl fmt::Display for RebootReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InternalError => write!(f, "Internal error"),
        }
    }
}

main!(start);
/// Entry point for pVM firmware.
pub fn start(fdt_address: u64, payload_start: u64, payload_size: u64, _arg3: u64) {
    // Limitations in this function:
    // - can't access non-pvmfw memory (only statically-mapped memory)
    // - can't access MMIO (therefore, no logging)
    // - deal with raw ABIs

    // SAFETY - These slices will be validated by main_wrapper().
    let fdt = unsafe { slice::from_raw_parts_mut(fdt_address as usize as *mut u8, FDT_MAX_SIZE) };
    let payload = unsafe {
        slice::from_raw_parts(payload_start as usize as *const u8, payload_size as usize)
    };

    match main_wrapper(fdt, payload) {
        Ok(_) => jump_to_payload(fdt_address, payload_start),
        Err(_) => reboot(),
    }

    // if we reach this point and return, vmbase::entry::rust_entry() will call power::shutdown().
}

/// Sets up the environment for main() and wraps its result for start().
///
/// Provide the abstractions necessary for start() to abort the pVM boot and for main() to run with
/// the assumption that its environment has been properly configured.
fn main_wrapper(fdt: &mut [u8], payload: &[u8]) -> Result<(), RebootReason> {
    // Limitations in this function:
    // - only access MMIO once (and while) it has been mapped and configured
    // - only perform logging once the logger has been initialized
    // - only access non-pvmfw memory once (and while) it has been mapped
    // - the location of inputs (in safe types) can't be trusted
    logger::init(LevelFilter::Info).map_err(|_| RebootReason::InternalError)?;

    if let Err(e) = mmio_guard::setup().map(|_| mmio_guard::map(console::BASE_ADDRESS)) {
        // We don't want to print to the UART when having failed to initialize it so use debug!()
        // to have logging exclusively for local builds that have tweaked the logger::init() call.
        debug!("Failed to configure UART: {e}");
        return Err(RebootReason::InternalError);
    }

    // This wrapper allows main() to be blissfully ignorant of platform details.
    crate::main(fdt, payload);

    Ok(())
}
