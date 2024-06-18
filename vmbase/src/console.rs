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
use core::{
    cell::OnceCell,
    fmt::{write, Arguments, Write},
};
use spin::mutex::SpinMutex;

const MAX_CONSOLES: usize = 4;

static CONSOLES: [SpinMutex<Option<Uart>>; MAX_CONSOLES] =
    [SpinMutex::new(None), SpinMutex::new(None), SpinMutex::new(None), SpinMutex::new(None)];
static ADDRESSES: [SpinMutex<OnceCell<usize>>; MAX_CONSOLES] = [
    SpinMutex::new(OnceCell::new()),
    SpinMutex::new(OnceCell::new()),
    SpinMutex::new(OnceCell::new()),
    SpinMutex::new(OnceCell::new()),
];

/// Initialises a new instance of the n-th UART driver and returns it.
fn create(n: usize) -> Option<Uart> {
    // SAFETY: ADDRESS is the base of the MMIO region for a UART and is mapped as device memory.
    Some(unsafe { Uart::new(*ADDRESSES[n].lock().get()?) })
}

/// Initialises the global instance(s) of the UART driver(s).
///
/// This must be called before using the `print!` and `println!` macros.
///
/// # Safety
///
/// This must be called with the bases of UARTs, mapped as device memory and (if necessary) shared
/// with the host as MMIO.
pub unsafe fn init(base_addresses: &[usize]) {
    for (i, base) in base_addresses.iter().enumerate() {
        ADDRESSES[i].lock().set(*base).unwrap();
        let uart = create(i).unwrap();
        CONSOLES[i].lock().replace(uart);
    }
}

/// Writes a string to the n-th console.
///
/// Panics if the n-th console was not initialized by calling [`init`] first.
pub(crate) fn write_str(n: usize, s: &str) {
    CONSOLES[n].lock().as_mut().unwrap().write_str(s).unwrap();
}

/// Writes a formatted string to the n-th console.
///
/// Panics if the n-th console was not initialized by calling [`init`] first.
pub(crate) fn write_args(n: usize, format_args: Arguments) {
    write(CONSOLES[n].lock().as_mut().unwrap(), format_args).unwrap();
}

/// Reinitializes the n-th UART driver and writes a string to it.
///
/// This is intended for use in situations where the UART may be in an unknown state or the global
/// instance may be locked, such as in an exception handler or panic handler.
pub fn emergency_write_str(n: usize, s: &str) {
    if let Some(mut uart) = create(n) {
        let _ = uart.write_str(s);
    }
}

/// Reinitializes the n-th UART driver and writes a formatted string to it.
///
/// This is intended for use in situations where the UART may be in an unknown state or the global
/// instance may be locked, such as in an exception handler or panic handler.
pub fn emergency_write_args(n: usize, format_args: Arguments) {
    if let Some(mut uart) = create(n) {
        let _ = write(&mut uart, format_args);
    }
}

/// Prints the given formatted string to the console, followed by a newline.
///
/// Panics if the console has not yet been initialized. May hang if used in an exception context;
/// use `eprintln!` instead.
macro_rules! println {
    () => ($crate::console::write_str(0, "\n"));
    ($($arg:tt)*) => ({
        $crate::console::write_args(0, format_args!($($arg)*))};
        $crate::console::write_str(0, "\n");
    );
}

pub(crate) use println; // Make it available in this crate.

/// Prints the given string to the console in an emergency, such as an exception handler.
///
/// Never panics.
#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => ($crate::console::emergency_write_args(0, format_args!($($arg)*)));
}

/// Prints the given string followed by a newline to the console in an emergency, such as an
/// exception handler.
///
/// Never panics.
#[macro_export]
macro_rules! eprintln {
    () => ($crate::console::emergency_write_str(0, "\n"));
    ($($arg:tt)*) => ({
        $crate::console::emergency_write_args(0, format_args!($($arg)*))};
        $crate::console::emergency_write_str(0, "\n");
    );
}
