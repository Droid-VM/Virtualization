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

use core::fmt::{write, Arguments, Write};
use uart8250::MmioUart8250;

const BASE_ADDRESS: usize = 0x3f8;
const CLOCK: usize = 11_059_200;
const BAUD_RATE: usize = 115200;

static mut CONSOLE: Option<MmioUart8250<'static>> = None;

pub fn create() -> MmioUart8250<'static> {
    let uart = MmioUart8250::new(BASE_ADDRESS);
    uart.init(CLOCK, BAUD_RATE);
    uart
}

pub fn init() {
    let uart = create();
    // TODO: Use a Mutex?
    unsafe {
        CONSOLE.replace(uart);
    }
}

pub fn write_str(s: &str) {
    // TODO: Use a Mutex?
    unsafe { CONSOLE.as_mut() }.unwrap().write_str(s).unwrap();
}

pub fn write_args(format_args: Arguments) {
    // TODO: Use a Mutex?
    write(unsafe { CONSOLE.as_mut() }.unwrap(), format_args).unwrap();
}

pub fn emergency_write_str(s: &str) {
    let mut uart = create();
    let _ = uart.write_str(s);
}
