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

mod console;
mod exceptions;
mod psci;

use console::emergency_write_str;
use core::panic::PanicInfo;
use psci::{system_off, system_reset};

/// Entry point for pVM firmware.
#[no_mangle]
pub extern "C" fn main() -> ! {
    console::init();
    console::write_str("before format_args\n");
    let args = format_args!("Hello world");
    console::write_str("before write\n");
    console::write_args(args);
    console::write_str("after write\n");
    //writeln!(&mut console, "Hello world").unwrap();

    system_off();
    #[allow(clippy::empty_loop)]
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    emergency_write_str("panic\n");
    system_reset();
    loop {}
}
