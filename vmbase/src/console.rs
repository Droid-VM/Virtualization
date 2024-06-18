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

static ADDRESS: SpinMutex<OnceCell<usize>> = SpinMutex::new(OnceCell::new());
static CONSOLE: SpinMutex<Option<Uart>> = SpinMutex::new(None);

/// Initialises a new instance of the UART driver and returns it.
fn create() -> Option<Uart> {
    // SAFETY: ADDRESS is the base of the MMIO region for a UART and is mapped as device memory.
    Some(unsafe { Uart::new(*ADDRESS.lock().get()?) })
}

/// Initialises the global instance of the UART driver.
///
/// This must be called before using the `print!` and `println!` macros.
///
/// # Safety
///
/// This must be called with the base of a UART, mapped as device memory and (if necessary) shared
/// with the host as MMIO.
pub unsafe fn init(base_address: usize) {
    ADDRESS.lock().set(base_address).unwrap();

    let uart = create().unwrap();

    CONSOLE.lock().replace(uart);
}

/// Writes a formatted string followed by a newline to the console.
///
/// Panics if [`init`] was not called first.
pub(crate) fn writeln(format_args: Arguments) {
    let mut guard = CONSOLE.lock();
    let uart = guard.as_mut().unwrap();

    write(uart, format_args).unwrap();
    let _ = uart.write_str("\n");
}

/// Reinitializes the UART driver and writes a formatted string followed by a newline to it.
///
/// This is intended for use in situations where the UART may be in an unknown state or the global
/// instance may be locked, such as in an exception handler or panic handler.
pub fn ewriteln(format_args: Arguments) {
    if let Some(mut uart) = create() {
        let _ = write(&mut uart, format_args);
        let _ = uart.write_str("\n");
    }
}

/// Prints the given formatted string to the console, followed by a newline.
///
/// Panics if the console has not yet been initialized. May hang if used in an exception context;
/// use `eprintln!` instead.
macro_rules! println {
    ($($arg:tt)*) => ($crate::console::writeln(format_args!($($arg)*)));
}

pub(crate) use println; // Make it available in this crate.

/// Prints the given string followed by a newline to the console in an emergency, such as an
/// exception handler.
///
/// Never panics.
#[macro_export]
macro_rules! eprintln {
    ($($arg:tt)*) => ($crate::console::ewriteln(format_args!($($arg)*)));
}
