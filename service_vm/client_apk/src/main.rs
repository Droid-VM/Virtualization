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

//! Main executable of Service VM client.

use anyhow::{ensure, Result};
use log::{error, info};
use std::{
    ffi::{c_void, CStr},
    mem::MaybeUninit,
    panic, ptr,
};
use vm_payload_bindgen::{
    attestation_status_t, AAttestationResult, AAttestationResult_resultToString,
    AVmPayload_freeAttestationResult, AVmPayload_getCertificateChainFromResult,
    AVmPayload_getPrivateKeyFromResult, AVmPayload_requestAttestation,
    AVmPayload_signWithAttestedKey,
};

/// Entry point of the Service VM client.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn AVmPayload_main() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("service_vm_client")
            .with_min_level(log::Level::Debug),
    );
    // Redirect panic messages to logcat.
    panic::set_hook(Box::new(|panic_info| {
        error!("{}", panic_info);
    }));
    if let Err(e) = try_main() {
        error!("failed with {:?}", e);
        std::process::exit(1);
    }
}

fn try_main() -> Result<()> {
    info!("Welcome to Service VM Client!");

    let mut res = MaybeUninit::uninit();
    let too_big_challenge = &[0u8; 66];
    // SAFETY: It is safe as we only read the challenge within its bounds and the
    // function does not retain any reference to it.
    let status = unsafe {
        AVmPayload_requestAttestation(
            too_big_challenge.as_ptr() as *const c_void,
            too_big_challenge.len(),
            res.as_mut_ptr(),
        )
    };
    ensure!(
        status == attestation_status_t::ATTESTATION_ERROR_INVALID_CHALLENGE,
        "Unexpeteced status: {:?}",
        status
    );
    log_status(status);
    // The data below is only a placeholder generated randomly with urandom
    let challenge = &[
        0x6c, 0xad, 0x52, 0x50, 0x15, 0xe7, 0xf4, 0x1d, 0xa5, 0x60, 0x7e, 0xd2, 0x7d, 0xf1, 0x51,
        0x67, 0xc3, 0x3e, 0x73, 0x9b, 0x30, 0xbd, 0x04, 0x20, 0x2e, 0xde, 0x3b, 0x1d, 0xc8, 0x07,
        0x11, 0x7b,
    ];
    info!("Sending challenge: {:?}", challenge);
    // SAFETY: It is safe as we only read the challenge within its bounds and the
    // function does not retain any reference to it.
    let status = unsafe {
        AVmPayload_requestAttestation(
            challenge.as_ptr() as *const c_void,
            challenge.len(),
            res.as_mut_ptr(),
        )
    };
    ensure!(status == attestation_status_t::ATTESTATION_OK, "Unexpeteced status: {:?}", status);
    log_status(status);
    // SAFETY: The result should be filled as the attestation succeeds.
    let res = unsafe { res.assume_init() };

    let cert_chain = get_certificate_chain(res)?;
    info!("Attestation result certificateChain = {:?}", cert_chain);

    let private_key = get_private_key(res)?;
    info!("Attestation result privateKey = {:?}", private_key);

    let message = b"Hello from Service VM client";
    info!("Signing messsage: {:?}", message);
    let signature = sign_with_attested_key(res, message)?;
    info!("Signature: {:?}", signature);

    // SAFETY: The result is returned by `AVmPayload_requestAttestation` and is only freed here.
    unsafe { AVmPayload_freeAttestationResult(res) };
    Ok(())
}

fn get_certificate_chain(res: *mut AAttestationResult) -> Result<Vec<u8>> {
    let size =
        // SAFETY: The result is returned by `AVmPayload_requestAttestation` and should be valid
        // before getting freed.
        unsafe { AVmPayload_getCertificateChainFromResult(res, ptr::null_mut(), 0) };
    let mut cert_chain = vec![0u8; size];
    // SAFETY: The result is returned by `AVmPayload_requestAttestation` and should be valid
    // before getting freed.
    unsafe {
        AVmPayload_getCertificateChainFromResult(
            res,
            cert_chain.as_mut_ptr() as *mut c_void,
            cert_chain.len(),
        );
    }
    Ok(cert_chain)
}

fn get_private_key(res: *mut AAttestationResult) -> Result<Vec<u8>> {
    let size =
        // SAFETY: The result is returned by `AVmPayload_requestAttestation` and should be valid
        // before getting freed.
        unsafe { AVmPayload_getPrivateKeyFromResult(res, ptr::null_mut(), 0) };
    let mut private_key = vec![0u8; size];
    // SAFETY: The result is returned by `AVmPayload_requestAttestation` and should be valid
    // before getting freed.
    unsafe {
        AVmPayload_getPrivateKeyFromResult(
            res,
            private_key.as_mut_ptr() as *mut c_void,
            private_key.len(),
        );
    }
    Ok(private_key)
}

fn sign_with_attested_key(res: *mut AAttestationResult, message: &[u8]) -> Result<Vec<u8>> {
    let size =
        // SAFETY: The result is returned by `AVmPayload_requestAttestation` and should be valid
        // before getting freed.
        unsafe { AVmPayload_signWithAttestedKey(res, message.as_ptr() as *const c_void, message.len(), ptr::null_mut(), 0) };
    let mut signature = vec![0u8; size];
    // SAFETY: The result is returned by `AVmPayload_requestAttestation` and should be valid
    // before getting freed.
    unsafe {
        AVmPayload_signWithAttestedKey(
            res,
            message.as_ptr() as *const c_void,
            message.len(),
            signature.as_mut_ptr() as *mut c_void,
            signature.len(),
        );
    }
    Ok(signature)
}

fn log_status(status: attestation_status_t) {
    // SAFETY: It's safe to convert the return status from `AVmPayload_requestAttestation` to
    // string.
    let message = unsafe { AAttestationResult_resultToString(status) };
    // SAFETY: The pointer returned by `AAttestationResult_resultToString` should point to a
    // valid string.
    let message = unsafe { CStr::from_ptr(message) };
    info!("Status: {:?}", message);
}
