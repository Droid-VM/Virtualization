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

//! Internal errors for pvmfw.

use crate::memory::MemoryTrackerError;
use alloc::string::String;
use avb_nostd::AvbImageVerifyError;
use core::fmt;

#[derive(Debug)]
pub(crate) enum RebootReason {
    /// A malformed BCC was received.
    InvalidBcc,
    /// An invalid configuration was appended to pvmfw.
    InvalidConfig,
    /// An unexpected internal error happened.
    InternalError(String),
    /// The provided FDT was invalid.
    InvalidFdt(String),
    /// The provided payload was invalid.
    InvalidPayload(usize),
    /// The provided ramdisk was invalid.
    InvalidRamdisk(MemoryTrackerError),
    /// Failed to verify the payload.
    PayloadVerificationError(AvbImageVerifyError),
}

impl fmt::Display for RebootReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidBcc => write!(f, "Invalid BCC."),
            Self::InvalidConfig => write!(f, "No valid configuration found."),
            Self::InternalError(message) => write!(f, "{message}"),
            Self::InvalidFdt(message) => write!(f, "{message}"),
            Self::InvalidPayload(payload_size) => {
                write!(f, "Invalid payload size: {:#x}", payload_size)
            }
            Self::InvalidRamdisk(e) => write!(f, "Failed to obtain the initrd range: {e}"),
            Self::PayloadVerificationError(e) => write!(f, "Failed to verify the payload: {e}"),
        }
    }
}
