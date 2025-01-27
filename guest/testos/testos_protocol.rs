// Copyright 2025, The Android Open Source Project
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

//! Test OS request and response format.

// #![cfg_attr(not(feature = "std"), no_std)]
#![no_std]

extern crate alloc;

/// Request message type for testos.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum Request {
    /// Request to read a byte range (offset, size) and send back a `Respone::Bytes` with the
    /// contents.
    ReadRange(usize, usize),
    /// Request to shutdown the VM.
    Shutdown,
}

/// Request message type for testos.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum Response {
    /// Bytes read by `Request::ReadRange`.
    Bytes(alloc::vec::Vec<u8>),
}
