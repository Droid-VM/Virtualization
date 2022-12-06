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
use ::avb::Ops;
use avb_bindgen::{
    AvbHashtreeErrorMode_AVB_HASHTREE_ERROR_MODE_EIO, AvbIOResult,
    AvbIOResult_AVB_IO_RESULT_ERROR_IO, AvbIOResult_AVB_IO_RESULT_OK, AvbOps,
    AvbSlotVerifyFlags_AVB_SLOT_VERIFY_FLAGS_NO_VBMETA_PARTITION,
};
use core::ptr::{null, null_mut};
use log::error;
pub use pvmfw_embedded_key::PUBLIC_KEY;

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

pub(crate) fn verify_payload(_kernel: &[u8], _ramdisk: Option<&[u8]>) -> Result<(), RebootReason> {
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
    Ops::new(avb_ops)
        .verify_slot(
            null(),
            null(),
            AvbSlotVerifyFlags_AVB_SLOT_VERIFY_FLAGS_NO_VBMETA_PARTITION,
            AvbHashtreeErrorMode_AVB_HASHTREE_ERROR_MODE_EIO,
        )
        .map_err(|e| {
            error!("Failed to verify the payload: {e}");
            RebootReason::PayloadVerificationError
        })?;
    Ok(())
}
