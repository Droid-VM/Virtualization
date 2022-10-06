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

//! pVM firmware.

#![no_main]
#![no_std]

mod exceptions;
mod helpers;
mod mmio;
mod smccc;

use core::fmt;
use log::{debug, error, info, trace, LevelFilter};
use vmbase::{console, logger, main, power::reboot};

#[derive(Debug, Clone)]
enum Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "")
    }
}

fn main(fdt_address: u64, payload_start: u64, payload_size: u64, arg3: u64) -> Result<(), Error> {
    info!("pVM firmware");
    debug!(
        "fdt_address={:#018x}, payload_start={:#018x}, payload_size={:#018x}, x3={:#018x}",
        fdt_address, payload_start, payload_size, arg3,
    );

    info!("Starting payload...");

    Ok(())
}

main!(main_wrapper);

/// Entry point for pVM firmware.
pub fn main_wrapper(fdt_address: u64, payload_start: u64, payload_size: u64, arg3: u64) {
    if logger::init(LevelFilter::Debug).is_err() {
        // We need to inform the hypervisor that the MMIO page containing the UART may be shared back.
    } else if let Err(e) = mmio::guard::setup().map(|_| mmio::guard::map(console::BASE_ADDRESS)) {
        // We don't want to print to the UART when having failed to initialize it so use trace!()
        // to have logging exclusively for local builds that have tweaked the logger::init() call.
        trace!("Failed to configure UART: {e}");
    } else if let Err(e) = main(fdt_address, payload_start, payload_size, arg3) {
        error!("Boot rejected: {e}");
    } else if let Err(e) = mmio::guard::unmap(console::BASE_ADDRESS) {
        error!("Failed to unmap UART: {e}");
    } else {
        jump_to_payload(fdt_address, payload_start);
    }

    reboot()
}

fn jump_to_payload(fdt_address: u64, payload_start: u64) {
    // Safe because this is a function we have implemented in assembly that matches its signature
    // here.
    unsafe {
        start_payload(fdt_address, payload_start);
    }
}

extern "C" {
    fn start_payload(fdt_address: u64, payload_start: u64) -> !;
}
