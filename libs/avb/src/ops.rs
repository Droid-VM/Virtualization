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

//! This module regroups methods related to AvbOps.

// TODO(b/256148034): Remove this when the feature is code complete.
#![allow(unused_imports)]

use alloc::{ffi::CString, vec::Vec};
use avb_bindgen::{
    self, avb_slot_verify, AvbHashtreeErrorMode, AvbOps, AvbSlotVerifyFlags,
    AvbSlotVerifyFlags_AVB_SLOT_VERIFY_FLAGS_ALLOW_VERIFICATION_ERROR,
    AvbSlotVerifyFlags_AVB_SLOT_VERIFY_FLAGS_NONE,
    AvbSlotVerifyFlags_AVB_SLOT_VERIFY_FLAGS_NO_VBMETA_PARTITION,
    AvbSlotVerifyFlags_AVB_SLOT_VERIFY_FLAGS_RESTART_CAUSED_BY_HASHTREE_CORRUPTION,
    AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_INVALID_ARGUMENT,
    AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_INVALID_METADATA,
    AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_IO,
    AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_OOM,
    AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_PUBLIC_KEY_REJECTED,
    AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_ROLLBACK_INDEX,
    AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_UNSUPPORTED_VERSION,
    AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_VERIFICATION,
    AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_OK,
};
use core::{
    ffi::c_char,
    fmt,
    ptr::{null, null_mut},
};
use log::debug;

/// Error code from AVB image verification.
#[derive(Clone, Debug)]
pub enum AvbImageVerifyError {
    /// AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_INVALID_ARGUMENT
    InvalidArgument,
    /// AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_INVALID_METADATA
    InvalidMetadata,
    /// AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_IO
    Io,
    /// AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_OOM
    Oom,
    /// AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_PUBLIC_KEY_REJECTED
    PublicKeyRejected,
    /// AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_ROLLBACK_INDEX
    RollbackIndex,
    /// AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_UNSUPPORTED_VERSION
    UnsupportedVersion,
    /// AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_VERIFICATION
    Verification,
    /// Unknown error.
    Unknown(u32),
}

fn to_avb_verify_result(result: u32) -> Result<(), AvbImageVerifyError> {
    #[allow(non_upper_case_globals)]
    match result {
        AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_OK => Ok(()),
        AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_INVALID_ARGUMENT => {
            Err(AvbImageVerifyError::InvalidArgument)
        }
        AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_INVALID_METADATA => {
            Err(AvbImageVerifyError::InvalidMetadata)
        }
        AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_IO => Err(AvbImageVerifyError::Io),
        AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_OOM => Err(AvbImageVerifyError::Oom),
        AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_PUBLIC_KEY_REJECTED => {
            Err(AvbImageVerifyError::PublicKeyRejected)
        }
        AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_ROLLBACK_INDEX => {
            Err(AvbImageVerifyError::RollbackIndex)
        }
        AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_UNSUPPORTED_VERSION => {
            Err(AvbImageVerifyError::UnsupportedVersion)
        }
        AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_ERROR_VERIFICATION => {
            Err(AvbImageVerifyError::Verification)
        }
        _ => Err(AvbImageVerifyError::Unknown(result)),
    }
}

impl fmt::Display for AvbImageVerifyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidArgument => write!(f, "Invalid parameters."),
            Self::InvalidMetadata => write!(f, "Invalid metadata."),
            Self::Io => write!(f, "I/O error while trying to load data or get a rollback index."),
            Self::Oom => write!(f, "Unable to allocate memory."),
            Self::PublicKeyRejected => write!(
                f,
                "Everything is verified correctly out but the public key is not accepted. \
                This includes the case where integrity data is not signed."
            ),
            Self::RollbackIndex => write!(f, "Rollback index is less than its stored value."),
            Self::UnsupportedVersion => write!(
                f,
                "Some of the metadata requires a newer version of libavb than what is in use."
            ),
            Self::Verification => write!(f, "Data does not verify."),
            Self::Unknown(e) => write!(f, "Unknown avb_slot_verify error '{e}'"),
        }
    }
}

/// A type that wraps avb_bindgen::AvbOps.
pub struct Ops(AvbOps);

impl Ops {
    /// Builds a new `Ops` object.
    pub fn new(avb_ops: AvbOps) -> Self {
        Self(avb_ops)
    }

    /// Invokes `avb_slot_verify` with the inner `AvbOps` object.
    pub fn verify_slot(
        &mut self,
        requested_partitions: &[CString],
        _ab_suffix: CString,
        _flags: AvbSlotVerifyFlags,
        _hashtree_error_mode: AvbHashtreeErrorMode,
    ) -> Result<(), AvbImageVerifyError> {
        let _requested_partitions_ptr: *const *const c_char =
            requested_partitions.iter().map(|s| s.as_ptr()).collect::<Vec<_>>().as_ptr();
        // TODO(b/256148034): Verify the kernel image with avb_slot_verify()
        // let result = unsafe {
        //     avb_slot_verify(
        //         &mut self.0,
        //         requested_partitions_ptr,
        //         ab_suffix.map_or(null(), |s| s.as_ptr()),
        //         flags,
        //         hashtree_error_mode,
        //         null_mut(),
        //     )
        // };
        let result = AvbSlotVerifyResult_AVB_SLOT_VERIFY_RESULT_OK;
        to_avb_verify_result(result)
    }
}
