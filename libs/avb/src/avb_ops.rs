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

//#![warn(unsafe_op_in_unsafe_fn)]
// TODO(b/256148034): Remove this when the feature is code complete.
#![allow(dead_code)]
#![allow(unused_imports)]

extern crate alloc;

use alloc::alloc::{alloc, dealloc, Layout};
use alloc::ffi::{CString, NulError};
use avb_bindgen::{
    self,
    avb_slot_verify,
    AvbHashtreeErrorMode_AVB_HASHTREE_ERROR_MODE_EIO,
    AvbIOResult,
    //   AvbIOResult_AVB_IO_RESULT_ERROR_OOM,
    AvbIOResult_AVB_IO_RESULT_ERROR_IO,
    AvbIOResult_AVB_IO_RESULT_OK,
    //   AvbIOResult_AVB_IO_RESULT_ERROR_NO_SUCH_PARTITION,
    //   AvbIOResult_AVB_IO_RESULT_ERROR_RANGE_OUTSIDE_PARTITION,
    //   AvbIOResult_AVB_IO_RESULT_ERROR_NO_SUCH_VALUE,
    //   AvbIOResult_AVB_IO_RESULT_ERROR_INVALID_VALUE_SIZE,
    //   AvbIOResult_AVB_IO_RESULT_ERROR_INSUFFICIENT_SPACE,
    AvbOps,
    AvbSlotVerifyFlags_AVB_SLOT_VERIFY_FLAGS_NO_VBMETA_PARTITION,
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
use core::{fmt, mem::size_of, ptr::null_mut};
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
    /// Invalid CStrings.
    InvalidCString(NulError),
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
            Self::InvalidCString(e) => write!(f, "Invalid CString: {e}."),
            Self::Unknown(e) => write!(f, "Unknown avb_slot_verify error '{e}'"),
        }
    }
}

/// Verifies that for the given image:
///  - The given public key is acceptable.
///  - The VBMeta struct is valid.
///  - The partitions of the image match the descriptors of the verified VBMeta struct.
/// Returns Ok if everything is verified correctly and the public key is accepted.
pub fn verify_image(image: &[u8], public_key: &[u8]) -> Result<(), AvbImageVerifyError> {
    Ops::new().verify_image(image, public_key)
}

/// A type that wraps avb_bindgen::AvbOps.
#[repr(transparent)]
struct Ops(AvbOps);

extern "C" fn read_is_device_unlocked(
    _ops: *mut AvbOps,
    out_is_unlocked: *mut bool,
) -> AvbIOResult {
    unsafe {
        *out_is_unlocked = false;
    }
    AvbIOResult_AVB_IO_RESULT_OK
}

extern "C" fn validate_vbmeta_public_key(
    _ops: *mut AvbOps,
    _public_key_data: *const u8,
    public_key_length: usize,
    _public_key_metadata: *const u8,
    _public_key_metadata_length: usize,
    _out_is_trusted: *mut bool,
) -> AvbIOResult {
    if public_key_length == 0 {
        return AvbIOResult_AVB_IO_RESULT_ERROR_IO;
    }
    AvbIOResult_AVB_IO_RESULT_OK
}

impl Ops {
    fn new() -> Self {
        let avb_ops = AvbOps {
            user_data: null_mut(),
            ab_ops: null_mut(),
            atx_ops: null_mut(),
            read_from_partition: None,
            get_preloaded_partition: None,
            write_to_partition: None,
            validate_vbmeta_public_key: Some(validate_vbmeta_public_key),
            read_rollback_index: None,
            write_rollback_index: None,
            read_is_device_unlocked: Some(read_is_device_unlocked),
            get_unique_guid_for_partition: None,
            get_size_of_partition: None,
            read_persistent_value: None,
            write_persistent_value: None,
            validate_public_key_for_partition: None,
        };
        Self(avb_ops)
    }

    fn verify_image(&mut self, image: &[u8], public_key: &[u8]) -> Result<(), AvbImageVerifyError> {
        debug!("AVB image: addr={:?}, size={:#x} ({1})", image.as_ptr(), image.len());
        debug!(
            "AVB public key: addr={:?}, size={:#x} ({1})",
            public_key.as_ptr(),
            public_key.len()
        );
        let ab_suffix = CString::new("_a").map_err(AvbImageVerifyError::InvalidCString)?;
        let result = unsafe {
            avb_slot_verify(
                &mut self.0,
                &image.as_ptr(),
                ab_suffix.as_ptr(),
                AvbSlotVerifyFlags_AVB_SLOT_VERIFY_FLAGS_NO_VBMETA_PARTITION,
                AvbHashtreeErrorMode_AVB_HASHTREE_ERROR_MODE_EIO,
                null_mut(),
            )
        };
        to_avb_verify_result(result)
    }
}
