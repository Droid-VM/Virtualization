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

//! Memory layout.

use vmbase::{layout, println};

pub fn print_addresses() {
    let dtb = layout::dtb_range();
    println!("dtb:        {}..{} ({} bytes)", dtb.start, dtb.end, dtb.end - dtb.start);
    let text = layout::text_range();
    println!("text:       {}..{} ({} bytes)", text.start, text.end, text.end - text.start);
    let rodata = layout::rodata_range();
    println!("rodata:     {}..{} ({} bytes)", rodata.start, rodata.end, rodata.end - rodata.start);
    println!("binary end: {}", layout::binary_end());
    let data = layout::data_range();
    println!(
        "data:       {}..{} ({} bytes, loaded at {})",
        data.start,
        data.end,
        data.end - data.start,
        layout::data_load_address(),
    );
    let bss = layout::bss_range();
    println!("bss:        {}..{} ({} bytes)", bss.start, bss.end, bss.end - bss.start);
    let boot_stack = layout::boot_stack_range();
    println!(
        "boot_stack: {}..{} ({} bytes)",
        boot_stack.start,
        boot_stack.end,
        boot_stack.end - boot_stack.start
    );
}
