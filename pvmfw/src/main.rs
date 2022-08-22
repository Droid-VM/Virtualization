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
mod smccc;

use core::fmt;

use vmbase::console::BASE_ADDRESS;
use vmbase::{main, println};

main!(main_wrapper);

#[derive(Debug, Clone)]
enum Error {
    /// Failed to configure the UART; no logs available.
    FailedUartSetup,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg = match self {
            Self::FailedUartSetup => "Failed to configure the UART",
        };
        write!(f, "{}", msg)
    }
}

fn main(fdt_address: u64, payload_start: u64, payload_size: u64, arg3: u64) -> Result<(), Error> {
    let uart = BASE_ADDRESS as u64;
    let mmio_granule = smccc::mmio_guard_info().map_err(|_| Error::FailedUartSetup)?;
    let uart_page = uart & !(mmio_granule - 1);
    smccc::mmio_guard_map(uart_page).map_err(|_| Error::FailedUartSetup)?;

    println!("pVM firmware");
    println!(
        "fdt_address={:#018x}, payload_start={:#018x}, payload_size={:#018x}, x3={:#018x}",
        fdt_address, payload_start, payload_size, arg3,
    );

    println!("Starting payload...");

    Ok(())
}

/// Entry point for pVM firmware.
pub fn main_wrapper(fdt_address: u64, payload_start: u64, payload_size: u64, arg3: u64) {
    match main(fdt_address, payload_start, payload_size, arg3) {
        Ok(()) => jump_to_payload(fdt_address, payload_start),
        Err(Error::FailedUartSetup) => (),
        #[allow(unreachable_patterns)]
        Err(e) => {
            println!("Boot rejected: {}", e);
        }
    }
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
