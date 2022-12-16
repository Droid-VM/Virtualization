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
#![allow(dead_code)]
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
    ptr, slice,
};

/// Error code from AVB image verification.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    // SAFETY: The raw pointer `out_is_unlocked` was created to point to a valid a `bool`,
    // and we are using `core::ptr::write` to dereference it, which performs bounds
    // checking like alignment and null checks.
    unsafe {
        ptr::write(out_is_unlocked, false);
    }
    AvbIOResult::AVB_IO_RESULT_OK
}

extern "C" fn read_from_partition(
    ops: *mut AvbOps,
    partition: *const c_char,
    offset: i64,
    num_bytes: usize,
    buffer: *mut c_void,
    out_num_read: *mut usize,
) -> AvbIOResult {
    let kernel = Payload::from_avb_ops_ptr(ops).kernel;
    // SAFETY: It is safe to copy the requested number of bytes to `buffer` as `buffer`
    // is created to point to the `num_bytes` of bytes in memory.
    let buffer_slice = unsafe { slice::from_raw_parts_mut(buffer as *mut u8, num_bytes) };
    if copy_data_to_dst(kernel, offset, buffer_slice).is_err() {
        return AvbIOResult::AVB_IO_RESULT_ERROR_IO;
    }
    // SAFETY: The raw pointer `out_num_read` was created to point to a valid a `usize` and
    // we are using `core::ptr::write` to dereference it, which performs bounds checking.
    unsafe {
        ptr::write(out_num_read, buffer_slice.len());
    }
    AvbIOResult::AVB_IO_RESULT_OK
}

fn copy_data_to_dst(src: &[u8], offset: i64, dst: &mut [u8]) -> Result<(), ()> {
    let start = to_copy_start(offset, src.len()).ok_or(())?;
    let end = start.checked_add(dst.len()).ok_or(())?;
    dst.copy_from_slice(src.get(start..end).ok_or(())?);
    Ok(())
}

fn to_copy_start(offset: i64, len: usize) -> Option<usize> {
    usize::try_from(offset)
        .ok()
        .or_else(|| isize::try_from(offset).ok().and_then(|v| len.checked_add_signed(v)))
}

extern "C" fn get_size_of_partition(
    ops: *mut AvbOps,
    _partition: *const c_char,
    out_size_num_bytes: *mut u64,
) -> AvbIOResult {
    let payload = Payload::from_avb_ops_ptr(ops);
    // SAFETY: The raw pointer `out_size_num_bytes` was created to point to a valid a `u64`
    // and we are using `core::ptr::write` to dereference it, which performs bounds checking.
    unsafe {
        ptr::write(out_size_num_bytes, payload.kernel.len() as u64);
    }
    AvbIOResult::AVB_IO_RESULT_OK
}

extern "C" fn read_rollback_index(
    ops: *mut AvbOps,
    rollback_index_location: usize,
    out_rollback_index: *mut u64,
) -> AvbIOResult {
    AvbIOResult::AVB_IO_RESULT_OK
}

extern "C" fn get_unique_guid_for_partition(
    ops: *mut AvbOps,
    partition: *const c_char,
    guid_buf: *mut c_char,
    guid_buf_size: usize,
) -> AvbIOResult {
    AvbIOResult::AVB_IO_RESULT_OK
}

extern "C" fn validate_public_key_for_partition(
    ops: *mut AvbOps,
    partition: *const c_char,
    public_key_data: *const u8,
    public_key_length: usize,
    public_key_metadata: *const u8,
    public_key_metadata_length: usize,
    out_is_trusted: *mut bool,
    out_rollback_index_location: *mut u32,
) -> AvbIOResult {
    // SAFETY: It is safe to create a slice with the given pointer and length as
    // `public_key_data` is a valid pointer and it points to an array of length
    // `public_key_length`, and `slice::from_raw_parts` also checks that the given
    // data pointer is aligned and non-null.
    let public_key = unsafe { slice::from_raw_parts(public_key_data, public_key_length) };
    let trusted_public_key = Payload::from_avb_ops_ptr(ops).trusted_public_key;
    // SAFETY: The raw pointer `out_is_trusted` was created to point to a valid a `bool`
    // and we are using `core::ptr::write` to dereference it, which performs bounds checking.
    unsafe {
        ptr::write(out_is_trusted, public_key == trusted_public_key);
    }
    AvbIOResult::AVB_IO_RESULT_OK
}

#[repr(C)]
struct Payload<'a> {
    kernel: &'a [u8],
    trusted_public_key: &'a [u8],
}

impl<'a> Payload<'a> {
    fn from_avb_ops_ptr(ops: *const AvbOps) -> &'a Self {
        // SAFETY: It is safe to cast the user_data to Payload as we have saved a pointer to a
        // valid value of Payload in user_data when creating AvbOps.
        unsafe {
            let payload = (*ops).user_data as *const Payload;
            &*payload
        }
    }
}

/// Verifies the payload (signed kernel + initrd) against the trusted public key.
pub fn verify_payload(kernel: &[u8], trusted_public_key: &[u8]) -> Result<(), AvbImageVerifyError> {
    let result = AvbSlotVerifyResult::AVB_SLOT_VERIFY_RESULT_OK;
    to_avb_verify_result(result)
}

/// TODO(b/256148034): This function is temporary as calling avb_slot_verify() is still
/// blocked due to missing C definitions. We should make this function `verify_payload`
/// once it's possible to call avb_slot_verify() in nostd.
fn verify_payload_temp(
    kernel: &[u8],
    trusted_public_key: &[u8],
) -> Result<(), AvbImageVerifyError> {
    let mut payload = Payload { kernel, trusted_public_key };
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
    // The partition name is only a placeholder here as the kernel image has only one partition,
    // we do not need the name to identify the requested partition.
    let requested_partition =
        CString::new("bootloader").map_err(|e| AvbImageVerifyError::InvalidCString)?;
    // NULL is needed to mark the end of the array for now.
    let requested_partitions: [*const c_char; 2] = [requested_partition.as_ptr(), ptr::null()];
    let ab_suffix = CString::new("").map_err(|e| AvbImageVerifyError::InvalidCString)?;

    // SAFETY: It is safe to call `avb_slot_verify()` as the pointer arguments (`ops`,
    // `requested_partitions` and `ab_suffix`) passed to the method are all valid and
    // initialized. The last argument `out_data` is allowed to be null so that nothing
    // will be written to it.
    let result = unsafe {
        avb_slot_verify(
            &mut avb_ops,
            requested_partitions.as_ptr(),
            ab_suffix.as_ptr(),
            AvbSlotVerifyFlags::AVB_SLOT_VERIFY_FLAGS_NO_VBMETA_PARTITION,
            AvbHashtreeErrorMode::AVB_HASHTREE_ERROR_MODE_RESTART_AND_INVALIDATE,
            /*out_data=*/ ptr::null_mut(),
        )
    };
    to_avb_verify_result(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use avb_bindgen::AvbFooter;
    use std::{fs, mem::size_of};

    const PUBLIC_KEY_RSA2048_PATH: &str = "data/testkey_rsa2048_pub.bin";
    const PUBLIC_KEY_RSA4096_PATH: &str = "data/testkey_rsa4096_pub.bin";

    /// This test uses the Microdroid payload compiled on the fly to check that
    /// the latest payload can be verified successfully.
    #[test]
    fn latest_valid_payload_is_verified_successfully() -> Result<()> {
        let kernel = load_latest_signed_kernel()?;
        let public_key = fs::read(PUBLIC_KEY_RSA4096_PATH)?;

        assert_eq!(Ok(()), verify_payload_temp(&kernel, &public_key));
        Ok(())
    }

    #[test]
    fn payload_with_empty_public_key_fails_verification() -> Result<()> {
        assert_payload_verification_fails(
            &load_latest_signed_kernel()?,
            /*trusted_public_key=*/ &[0u8; 0],
            AvbImageVerifyError::PublicKeyRejected,
        )
    }

    #[test]
    fn payload_with_an_invalid_public_key_fails_verification() -> Result<()> {
        assert_payload_verification_fails(
            &load_latest_signed_kernel()?,
            /*trusted_public_key=*/ &[0u8; 512],
            AvbImageVerifyError::PublicKeyRejected,
        )
    }

    #[test]
    fn payload_with_a_different_valid_public_key_fails_verification() -> Result<()> {
        assert_payload_verification_fails(
            &load_latest_signed_kernel()?,
            &fs::read(PUBLIC_KEY_RSA2048_PATH)?,
            AvbImageVerifyError::PublicKeyRejected,
        )
    }

    #[test]
    fn unsigned_kernel_fails_verification() -> Result<()> {
        assert_payload_verification_fails(
            &fs::read("unsigned_test.img")?,
            &fs::read(PUBLIC_KEY_RSA4096_PATH)?,
            AvbImageVerifyError::Io,
        )
    }

    #[test]
    fn tampered_kernel_fails_verification() -> Result<()> {
        let mut kernel = load_latest_signed_kernel()?;
        let tampered_header = [0u8; 100];
        assert!(
            tampered_header != kernel[..tampered_header.len()],
            "Tampered header should be different with the original kernel."
        );
        kernel[..tampered_header.len()].copy_from_slice(&tampered_header);

        assert_payload_verification_fails(
            &kernel,
            &fs::read(PUBLIC_KEY_RSA4096_PATH)?,
            AvbImageVerifyError::Verification,
        )
    }

    fn assert_payload_verification_fails(
        kernel: &[u8],
        trusted_public_key: &[u8],
        expected_error: AvbImageVerifyError,
    ) -> Result<()> {
        assert_eq!(Err(expected_error), verify_payload_temp(kernel, trusted_public_key));
        Ok(())
    }

    fn load_latest_signed_kernel() -> Result<Vec<u8>> {
        Ok(fs::read("microdroid_kernel")?)
    }
}
