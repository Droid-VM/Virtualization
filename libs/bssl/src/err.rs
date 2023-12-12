// Copyright 2023, The Android Open Source Project
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

//! Wrappers of the error handling functions in BoringSSL err.h.

use bssl_avf_error::{CipherError, EcError, EcdsaError, GlobalError, ReasonCode};
use bssl_ffi::{
    self, ERR_get_error_line, ERR_lib_error_string, ERR_reason_error_string, ERR_GET_LIB_RUST,
    ERR_GET_REASON_RUST,
};
use core::ffi::{c_char, CStr};
use core::ptr;
use log::error;

const NO_ERROR_REASON_CODE: i32 = 0;

/// Returns the reason code for the least recent error and removes that
/// error from the error queue.
pub(crate) fn get_error_reason_code() -> ReasonCode {
    let mut file = ptr::null();
    let mut line = 0;
    // SAFETY: This function only reads the error queue and writes to the given
    // pointers. It doesn't retain any references to the pointers.
    let packed_error = unsafe { ERR_get_error_line(&mut file, &mut line) };
    // SAFETY: Any non-null result is expected to point to a global const C string.
    let file = unsafe { as_static_cstr(file) };
    error!(
        "BoringSSL error: {}:{}: lib = {}, reason = {}",
        file.map(|s| s.to_string_lossy()).unwrap_or_else(|| "<unknown file>".into()),
        line,
        lib_error_string(packed_error)
            .map(|s| s.to_string_lossy())
            .unwrap_or_else(|| "<unknown library>".into()),
        reason_error_string(packed_error)
            .map(|s| s.to_string_lossy())
            .unwrap_or_else(|| "<unknown reason>".into()),
    );

    let reason = get_reason(packed_error);
    let lib = get_lib(packed_error);
    map_to_reason_code(reason, lib)
}

fn lib_error_string(packed_error: u32) -> Option<&'static CStr> {
    // SAFETY: This function only reads the given error code and returns a
    // pointer to a static string.
    let p = unsafe { ERR_lib_error_string(packed_error) };
    // SAFETY: Any non-null result is expected to point to a global const C string.
    unsafe { as_static_cstr(p) }
}

fn reason_error_string(packed_error: u32) -> Option<&'static CStr> {
    // SAFETY: This function only reads the given error code and returns a
    // pointer to a static string.
    let p = unsafe { ERR_reason_error_string(packed_error) };
    // SAFETY: Any non-null result is expected to point to a global const C string.
    unsafe { as_static_cstr(p) }
}

/// Casts a C string pointer to a static non-mutable reference.
///
/// # Safety
///
/// The caller needs to ensure that the pointer is null or points to a valid C string that is
/// valid for the entire lifetime of the program.
unsafe fn as_static_cstr(p: *const c_char) -> Option<&'static CStr> {
    if p.is_null() {
        None
    } else {
        // Safety: Safe given the requirements of this function.
        Some(unsafe { CStr::from_ptr(p) })
    }
}

fn get_reason(packed_error: u32) -> i32 {
    // SAFETY: This function only reads the given error code.
    unsafe { ERR_GET_REASON_RUST(packed_error) }
}

/// Returns the library code for the error.
fn get_lib(packed_error: u32) -> i32 {
    // SAFETY: This function only reads the given error code.
    unsafe { ERR_GET_LIB_RUST(packed_error) }
}

fn map_to_reason_code(reason: i32, lib: i32) -> ReasonCode {
    if reason == NO_ERROR_REASON_CODE {
        return ReasonCode::NoError;
    }
    map_global_reason_code(reason)
        .map(ReasonCode::Global)
        .or_else(|| map_library_reason_code(reason, lib))
        .unwrap_or(ReasonCode::Unknown(reason, lib))
}

/// Global errors may occur in any library.
fn map_global_reason_code(reason: i32) -> Option<GlobalError> {
    let reason = match reason {
        bssl_ffi::ERR_R_FATAL => GlobalError::Fatal,
        bssl_ffi::ERR_R_MALLOC_FAILURE => GlobalError::MallocFailure,
        bssl_ffi::ERR_R_SHOULD_NOT_HAVE_BEEN_CALLED => GlobalError::ShouldNotHaveBeenCalled,
        bssl_ffi::ERR_R_PASSED_NULL_PARAMETER => GlobalError::PassedNullParameter,
        bssl_ffi::ERR_R_INTERNAL_ERROR => GlobalError::InternalError,
        bssl_ffi::ERR_R_OVERFLOW => GlobalError::Overflow,
        _ => return None,
    };
    Some(reason)
}

fn map_library_reason_code(reason: i32, lib: i32) -> Option<ReasonCode> {
    u32::try_from(lib).ok().and_then(|x| match x {
        bssl_ffi::ERR_LIB_CIPHER => map_cipher_reason_code(reason).map(ReasonCode::Cipher),
        bssl_ffi::ERR_LIB_EC => map_ec_reason_code(reason).map(ReasonCode::Ec),
        bssl_ffi::ERR_LIB_ECDSA => map_ecdsa_reason_code(reason).map(ReasonCode::Ecdsa),
        _ => None,
    })
}

fn map_cipher_reason_code(reason: i32) -> Option<CipherError> {
    let error = match reason {
        bssl_ffi::CIPHER_R_AES_KEY_SETUP_FAILED => CipherError::AesKeySetupFailed,
        bssl_ffi::CIPHER_R_BAD_DECRYPT => CipherError::BadDecrypt,
        bssl_ffi::CIPHER_R_BAD_KEY_LENGTH => CipherError::BadKeyLength,
        bssl_ffi::CIPHER_R_BUFFER_TOO_SMALL => CipherError::BufferTooSmall,
        bssl_ffi::CIPHER_R_CTRL_NOT_IMPLEMENTED => CipherError::CtrlNotImplemented,
        bssl_ffi::CIPHER_R_CTRL_OPERATION_NOT_IMPLEMENTED => {
            CipherError::CtrlOperationNotImplemented
        }
        bssl_ffi::CIPHER_R_DATA_NOT_MULTIPLE_OF_BLOCK_LENGTH => {
            CipherError::DataNotMultipleOfBlockLength
        }
        bssl_ffi::CIPHER_R_INITIALIZATION_ERROR => CipherError::InitializationError,
        bssl_ffi::CIPHER_R_INPUT_NOT_INITIALIZED => CipherError::InputNotInitialized,
        bssl_ffi::CIPHER_R_INVALID_AD_SIZE => CipherError::InvalidAdSize,
        bssl_ffi::CIPHER_R_INVALID_KEY_LENGTH => CipherError::InvalidKeyLength,
        bssl_ffi::CIPHER_R_INVALID_NONCE_SIZE => CipherError::InvalidNonceSize,
        bssl_ffi::CIPHER_R_INVALID_OPERATION => CipherError::InvalidOperation,
        bssl_ffi::CIPHER_R_IV_TOO_LARGE => CipherError::IvTooLarge,
        bssl_ffi::CIPHER_R_NO_CIPHER_SET => CipherError::NoCipherSet,
        bssl_ffi::CIPHER_R_OUTPUT_ALIASES_INPUT => CipherError::OutputAliasesInput,
        bssl_ffi::CIPHER_R_TAG_TOO_LARGE => CipherError::TagTooLarge,
        bssl_ffi::CIPHER_R_TOO_LARGE => CipherError::TooLarge,
        bssl_ffi::CIPHER_R_WRONG_FINAL_BLOCK_LENGTH => CipherError::WrongFinalBlockLength,
        bssl_ffi::CIPHER_R_NO_DIRECTION_SET => CipherError::NoDirectionSet,
        bssl_ffi::CIPHER_R_INVALID_NONCE => CipherError::InvalidNonce,
        _ => return None,
    };
    Some(error)
}

fn map_ec_reason_code(reason: i32) -> Option<EcError> {
    let error = match reason {
        bssl_ffi::EC_R_BUFFER_TOO_SMALL => EcError::BufferTooSmall,
        bssl_ffi::EC_R_COORDINATES_OUT_OF_RANGE => EcError::CoordinatesOutOfRange,
        bssl_ffi::EC_R_D2I_ECPKPARAMETERS_FAILURE => EcError::D2IEcpkparametersFailure,
        bssl_ffi::EC_R_EC_GROUP_NEW_BY_NAME_FAILURE => EcError::EcGroupNewByNameFailure,
        bssl_ffi::EC_R_GROUP2PKPARAMETERS_FAILURE => EcError::Group2PkparametersFailure,
        bssl_ffi::EC_R_I2D_ECPKPARAMETERS_FAILURE => EcError::I2DEcpkparametersFailure,
        bssl_ffi::EC_R_INCOMPATIBLE_OBJECTS => EcError::IncompatibleObjects,
        bssl_ffi::EC_R_INVALID_COMPRESSED_POINT => EcError::InvalidCompressedPoint,
        bssl_ffi::EC_R_INVALID_COMPRESSION_BIT => EcError::InvalidCompressionBit,
        bssl_ffi::EC_R_INVALID_ENCODING => EcError::InvalidEncoding,
        bssl_ffi::EC_R_INVALID_FIELD => EcError::InvalidField,
        bssl_ffi::EC_R_INVALID_FORM => EcError::InvalidForm,
        bssl_ffi::EC_R_INVALID_GROUP_ORDER => EcError::InvalidGroupOrder,
        bssl_ffi::EC_R_INVALID_PRIVATE_KEY => EcError::InvalidPrivateKey,
        bssl_ffi::EC_R_MISSING_PARAMETERS => EcError::MissingParameters,
        bssl_ffi::EC_R_MISSING_PRIVATE_KEY => EcError::MissingPrivateKey,
        bssl_ffi::EC_R_NON_NAMED_CURVE => EcError::NonNamedCurve,
        bssl_ffi::EC_R_NOT_INITIALIZED => EcError::NotInitialized,
        bssl_ffi::EC_R_PKPARAMETERS2GROUP_FAILURE => EcError::Pkparameters2GroupFailure,
        bssl_ffi::EC_R_POINT_AT_INFINITY => EcError::PointAtInfinity,
        bssl_ffi::EC_R_POINT_IS_NOT_ON_CURVE => EcError::PointIsNotOnCurve,
        bssl_ffi::EC_R_SLOT_FULL => EcError::SlotFull,
        bssl_ffi::EC_R_UNDEFINED_GENERATOR => EcError::UndefinedGenerator,
        bssl_ffi::EC_R_UNKNOWN_GROUP => EcError::UnknownGroup,
        bssl_ffi::EC_R_UNKNOWN_ORDER => EcError::UnknownOrder,
        bssl_ffi::EC_R_WRONG_ORDER => EcError::WrongOrder,
        bssl_ffi::EC_R_BIGNUM_OUT_OF_RANGE => EcError::BignumOutOfRange,
        bssl_ffi::EC_R_WRONG_CURVE_PARAMETERS => EcError::WrongCurveParameters,
        bssl_ffi::EC_R_DECODE_ERROR => EcError::DecodeError,
        bssl_ffi::EC_R_ENCODE_ERROR => EcError::EncodeError,
        bssl_ffi::EC_R_GROUP_MISMATCH => EcError::GroupMismatch,
        bssl_ffi::EC_R_INVALID_COFACTOR => EcError::InvalidCofactor,
        bssl_ffi::EC_R_PUBLIC_KEY_VALIDATION_FAILED => EcError::PublicKeyValidationFailed,
        bssl_ffi::EC_R_INVALID_SCALAR => EcError::InvalidScalar,
        _ => return None,
    };
    Some(error)
}

fn map_ecdsa_reason_code(reason: i32) -> Option<EcdsaError> {
    let error = match reason {
        bssl_ffi::ECDSA_R_BAD_SIGNATURE => EcdsaError::BadSignature,
        bssl_ffi::ECDSA_R_MISSING_PARAMETERS => EcdsaError::MissingParameters,
        bssl_ffi::ECDSA_R_NEED_NEW_SETUP_VALUES => EcdsaError::NeedNewSetupValues,
        bssl_ffi::ECDSA_R_NOT_IMPLEMENTED => EcdsaError::NotImplemented,
        bssl_ffi::ECDSA_R_RANDOM_NUMBER_GENERATION_FAILED => {
            EcdsaError::RandomNumberGenerationFailed
        }
        bssl_ffi::ECDSA_R_ENCODE_ERROR => EcdsaError::EncodeError,
        bssl_ffi::ECDSA_R_TOO_MANY_ITERATIONS => EcdsaError::TooManyIterations,
        _ => return None,
    };
    Some(error)
}
