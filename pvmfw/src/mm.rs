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

//! Memory management code.

use crate::console::emergency_write_str;

#[no_mangle]
pub static mut mm_config: Config =
    Config { ttbr_el2: 0, vtcr_el2: 0, mair_el2: 0, tcr_el2: 0, sctlr_el1: 0 };

#[repr(C)]
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Config {
    ttbr_el2: u64,
    vtcr_el2: u64,
    mair_el2: u64,
    tcr_el2: u64,
    sctlr_el1: u64,
}

/// Initialises the page table.
///
/// This function is called with the MMU disabled, so must not make any unaligned accesses.
#[no_mangle]
pub extern "C" fn init_mm() {
    emergency_write_str("init_mm\n");
}
