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

//! Console driver for 8250 UART.

use crate::uart::Uart;
use core::fmt::{write, Arguments, Write};

const BASE_ADDRESS: usize = 0x3f8;

static mut CONSOLE: Option<Uart> = None;

/// Initialise a new instance of the UART driver and return it.
pub fn create() -> Uart {
    unsafe { Uart::new(BASE_ADDRESS) }
}

/// Initialise the global instance of the UART driver. This must be called before using
/// the `print!` and `println!` macros.
pub fn init() {
    let uart = create();
    // TODO: Use a Mutex?
    unsafe {
        CONSOLE.replace(uart);
    }
}

/// Write a string to the console.
///
/// Panics if [`init`] was not called first.
pub fn write_str(s: &str) {
    // TODO: Use a Mutex?
    unsafe { CONSOLE.as_mut() }.unwrap().write_str(s).unwrap();
}

/// Write a formatted string to the console.
///
/// Panics if [`init`] was not called first.
#[allow(unused)]
pub fn write_args(format_args: Arguments) {
    // TODO: Use a Mutex?
    write(unsafe { CONSOLE.as_mut() }.unwrap(), format_args).unwrap();
}

/// Reinitialise the UART driver and write a string to it.
///
/// This is intended for use in situations where the UART may be in an unknown state or the global
/// instance may be locked, such as in an exception handler or panic handler.
pub fn emergency_write_str(s: &str) {
    let mut uart = create();
    let _ = uart.write_str(s);
}
