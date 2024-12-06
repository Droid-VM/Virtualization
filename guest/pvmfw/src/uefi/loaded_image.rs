// Copyright 2024, The Android Open Source Project
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

//! Support for EFI Loaded Image Protocol.

use core::ptr::{null, null_mut};
use uefi_raw::protocol::loaded_image::LoadedImageProtocol;
use uefi_raw::table::boot::MemoryType;

const LOADED_IMAGE_PROTOCOL_REVISION: u32 = 0x1000;

pub const fn init_loaded_image_protocol() -> LoadedImageProtocol {
    LoadedImageProtocol {
        revision: LOADED_IMAGE_PROTOCOL_REVISION,
        parent_handle: null_mut(),
        system_table: null(),

        device_handle: null_mut(),
        file_path: null(),

        reserved: null(),

        load_options_size: 0,
        load_options: null(),

        image_base: null(),
        image_size: 0,
        image_code_type: MemoryType::LOADER_CODE,
        image_data_type: MemoryType::LOADER_DATA,
        unload: None,
    }
}
