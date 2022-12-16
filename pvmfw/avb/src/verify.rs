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

//! This module handles the pvmfw payload verification.

// TODO(b/256148034): Remove these once it's possible to call `avb_slot_verify` in pvmfw.
#![allow(unused_imports)]
#![allow(unused_variables)]

use alloc::ffi::CString;
use avb_bindgen::{
    avb_slot_verify, AvbHashtreeErrorMode, AvbIOResult, AvbOps, AvbSlotVerifyFlags,
    AvbSlotVerifyResult,
};
use core::{
    ffi::{c_char, c_void},
    fmt,
    mem::transmute,
    ptr,
};

/// Error code from AVB image verification.
#[derive(Clone, Debug)]
pub enum AvbImageVerifyError {
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
    /// Invalid C String
    InvalidCString,
}

impl fmt::Display for AvbImageVerifyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidArgument => write!(f, "Invalid parameters."),
            Self::InvalidMetadata => write!(f, "Invalid metadata."),
            Self::Io => write!(f, "I/O error while trying to load data or get a rollback index."),
            Self::Oom => write!(f, "Unable to allocate memory."),
            Self::PublicKeyRejected => write!(f, "Public key rejected or data not signed."),
            Self::RollbackIndex => write!(f, "Rollback index is less than its stored value."),
            Self::UnsupportedVersion => write!(
                f,
                "Some of the metadata requires a newer version of libavb than what is in use."
            ),
            Self::Verification => write!(f, "Data does not verify."),
            Self::InvalidCString => write!(f, "Invalid C String"),
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

extern "C" fn read_is_device_unlocked(
    _ops: *mut AvbOps,
    out_is_unlocked: *mut bool,
) -> AvbIOResult {
    // SAFETY: The raw pointer `out_is_unlocked` was created to point to a valid a boolean, so
    // we know the pointer is not null and the memory it points to is valid and has the layout
    // of a `bool`, and we are using `core::ptr::write` to dereference it, which performs bounds
    // checking.
    unsafe {
        ptr::write(out_is_unlocked, false);
    }
    AvbIOResult::AVB_IO_RESULT_OK
}

unsafe extern "C" fn read_from_partition(
    ops: *mut AvbOps,
    partition: *const c_char,
    offset: i64,
    num_bytes: usize,
    buffer: *mut c_void,
    out_num_read: *mut usize,
) -> AvbIOResult {
    AvbIOResult::AVB_IO_RESULT_OK
}

unsafe extern "C" fn get_size_of_partition(
    ops: *mut AvbOps,
    partition: *const c_char,
    out_size_num_bytes: *mut u64,
) -> AvbIOResult {
    AvbIOResult::AVB_IO_RESULT_OK
}

unsafe extern "C" fn read_rollback_index(
    ops: *mut AvbOps,
    rollback_index_location: usize,
    out_rollback_index: *mut u64,
) -> AvbIOResult {
    AvbIOResult::AVB_IO_RESULT_OK
}

unsafe extern "C" fn get_unique_guid_for_partition(
    ops: *mut AvbOps,
    partition: *const c_char,
    guid_buf: *mut c_char,
    guid_buf_size: usize,
) -> AvbIOResult {
    AvbIOResult::AVB_IO_RESULT_OK
}

unsafe extern "C" fn validate_public_key_for_partition(
    ops: *mut AvbOps,
    partition: *const c_char,
    public_key_data: *const u8,
    public_key_length: usize,
    public_key_metadata: *const u8,
    public_key_metadata_length: usize,
    out_is_trusted: *mut bool,
    out_rollback_index_location: *mut u32,
) -> AvbIOResult {
    AvbIOResult::AVB_IO_RESULT_OK
}

#[repr(C, packed)]
struct Payload {
    kernel_start: *const u8,
    kernel_size: usize,
}

/// Verifies the payload (signed kernel + initrd) against the trusted public key.
pub fn verify_payload(_public_key: &[u8]) -> Result<(), AvbImageVerifyError> {
    let result = AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_OK;
    to_avb_verify_result(result)
}

/// TODO(b/256148034): This function is temporary as calling avb_slot_verify() is still
/// blocked due to missing C definitions. We should make this function `verify_payload`
/// one once it's possible to call avb_slot_verify() in nostd.
#[cfg(test)]
fn verify_payload_temp(mut payload: Payload) -> Result<(), AvbImageVerifyError> {
    let mut avb_ops = AvbOps {
        user_data: &mut payload as *mut _ as *mut c_void,
        ab_ops: ptr::null_mut(),
        atx_ops: ptr::null_mut(),
        read_from_partition: Some(read_from_partition),
        get_preloaded_partition: None,
        write_to_partition: None,
        validate_vbmeta_public_key: None,
        read_rollback_index: Some(read_rollback_index),
        write_rollback_index: None,
        read_is_device_unlocked: Some(read_is_device_unlocked),
        get_unique_guid_for_partition: Some(get_unique_guid_for_partition),
        get_size_of_partition: Some(get_size_of_partition),
        read_persistent_value: None,
        write_persistent_value: None,
        validate_public_key_for_partition: Some(validate_public_key_for_partition),
    };
    // TODO(b/262853105): Rename the kernel partition name to "kernel"
    let requested_partition =
        CString::new("bootloader").map_err(|e| AvbImageVerifyError::InvalidCString)?;
    let requested_partitions: [*const c_char; 1] = [requested_partition.as_ptr()];
    let result = unsafe {
        avb_slot_verify(
            &mut avb_ops,
            requested_partitions.as_ptr(),
            ptr::null(),
            AvbSlotVerifyFlags::AVB_SLOT_VERIFY_FLAGS_NO_VBMETA_PARTITION,
            AvbHashtreeErrorMode::AVB_HASHTREE_ERROR_MODE_RESTART_AND_INVALIDATE,
            ptr::null_mut(),
        )
    };
    to_avb_verify_result(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Read;

    // TODO(b/256148034): Test verification succeeds with valid payload later.
    #[test]
    fn valid_payload_is_verified_successfully() {
        let mut kernel_file =
            File::open("testdata/microdroid_kernel").expect("Cannot open kernel file");
        let kernel_size = kernel_file.metadata().expect("Cannot read metadata").len();
        let mut kernel =
            vec![0u8; kernel_size.try_into().expect("Cannot convert kernel size to usize")];
        kernel_file.read_exact(&mut kernel).expect("Cannot read the kernel");
        let payload = Payload { kernel_start: kernel.as_ptr(), kernel_size: kernel.len() };

        assert!(verify_payload_temp(payload).is_ok());
    }
}
