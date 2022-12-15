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

//! Image verification.

use crate::entry::RebootReason;
use alloc::ffi::CString;
use avb_bindgen::{AvbHashtreeErrorMode, AvbOps, AvbSlotVerifyFlags, AvbSlotVerifyResult};
use core::{fmt, ptr::null_mut};
use log::{debug, error};

pub use pvmfw_embedded_key::PUBLIC_KEY;

/// Error code from AVB image verification.
#[derive(Clone, Copy, Debug)]
enum AvbImageVerifyError {
    /// AVB_SLOT_VERIFY_RESULT_ERROR_INVALID_ARGUMENT
    InvalidArgument,
    /// AVB_SLOT_VERIFY_RESULT_ERROR_INVALID_METADATA
    InvalidMetadata,
    /// AVB_SLOT_VERIFY_RESULT_ERROR_IO
    Io,
    /// AVB_SLOT_VERIFY_RESULT_ERROR_OOM
    Oom,
    /// AVB_SLOT_VERIFY_RESULT_ERROR_PUBLIC_KEY_REJECTED
    PublicKeyRejected,
    /// AVB_SLOT_VERIFY_RESULT_ERROR_ROLLBACK_INDEX
    RollbackIndex,
    /// AVB_SLOT_VERIFY_RESULT_ERROR_UNSUPPORTED_VERSION
    UnsupportedVersion,
    /// AVB_SLOT_VERIFY_RESULT_ERROR_VERIFICATION
    Verification,
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
        }
    }
}

fn to_avb_verify_result(result: AvbSlotVerifyResult) -> Result<(), AvbImageVerifyError> {
    match result {
        AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_OK => Ok(()),
        AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_ERROR_INVALID_ARGUMENT => {
            Err(AvbImageVerifyError::InvalidArgument)
        }
        AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_ERROR_INVALID_METADATA => {
            Err(AvbImageVerifyError::InvalidMetadata)
        }
        AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_ERROR_IO => Err(AvbImageVerifyError::Io),
        AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_ERROR_OOM => Err(AvbImageVerifyError::Oom),
        AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_ERROR_PUBLIC_KEY_REJECTED => {
            Err(AvbImageVerifyError::PublicKeyRejected)
        }
        AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_ERROR_ROLLBACK_INDEX => {
            Err(AvbImageVerifyError::RollbackIndex)
        }
        AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_ERROR_UNSUPPORTED_VERSION => {
            Err(AvbImageVerifyError::UnsupportedVersion)
        }
        AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_ERROR_VERIFICATION => {
            Err(AvbImageVerifyError::Verification)
        }
    }
}

/// Verifies the payload (signed kernel + initrd) against the trusted public key.
pub(crate) fn verify_payload() -> Result<(), RebootReason> {
    let mut _avb_ops = AvbOps {
        user_data: null_mut(),
        ab_ops: null_mut(),
        atx_ops: null_mut(),
        read_from_partition: None,
        get_preloaded_partition: None,
        write_to_partition: None,
        validate_vbmeta_public_key: None,
        read_rollback_index: None,
        write_rollback_index: None,
        read_is_device_unlocked: None,
        get_unique_guid_for_partition: None,
        get_size_of_partition: None,
        read_persistent_value: None,
        write_persistent_value: None,
        validate_public_key_for_partition: None,
    };
    let _requested_partitions = [CString::new("bootloader")
        .map_err(|e| {
            error!("Invalid CString for requested partitions: {e}");
            RebootReason::InternalError
        })?
        .as_ptr()];
    let _ab_suffix = CString::new("_a").map_err(|e| {
        error!("Invalid CString for ab_suffix: {e}");
        RebootReason::InternalError
    })?;
    let flags = AvbSlotVerifyFlags::AVB_SLOT_VERIFY_FLAGS_NO_VBMETA_PARTITION;
    let hashtree_error_mode = AvbHashtreeErrorMode::AVB_HASHTREE_ERROR_MODE_EIO;
    debug!("flags: {:?}", flags);
    debug!("hashtree_error_mode: {:?}", hashtree_error_mode);
    // TODO(b/256148034): Verify the kernel image with avb_slot_verify()
    // let result = unsafe {
    //     avb_slot_verify(
    //         &mut avb_ops,
    //         requested_partitions.as_ptr(),
    //         ab_suffix.as_ptr(),
    //         flags,
    //         hashtree_error_mode,
    //         null_mut(),
    //     )
    // };
    let result = AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_OK;
    to_avb_verify_result(result).map_err(|e| {
        error!("Failed to verify the payload: {e}");
        RebootReason::PayloadVerificationError
    })
}
