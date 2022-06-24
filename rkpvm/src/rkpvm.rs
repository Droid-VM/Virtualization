/*
 * Copyright (C) 2022 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! An RPK service VM for VMs.

use anyhow::Result;
use binder_common::new_binder_exception;
use cert_request_validator::bcc;
use cert_request_validator::publickey::PublicKey;
use log::debug;
use openssl::pkey::PKey;
use rkpvmconnection::respond;

use rkpvm_aidl_interface::aidl::com::android::rkpvm::IRkpVmService::{
    BnRkpVmService, IRkpVmService,
};
use rkpvm_aidl_interface::binder::{
    BinderFeatures, ExceptionCode, Interface, Result as BinderResult, Strong,
};

/// Constructs a binder object that implements IRkpVmService.
pub fn new_binder() -> Result<Strong<dyn IRkpVmService>> {
    let service = RkpVmService {};
    Ok(BnRkpVmService::new_binder(service, BinderFeatures::default()))
}

struct RkpVmService {}

impl Interface for RkpVmService {}

fn get_remote_attestation_key(
    bcc: &[u8],
    ephemeral_key: &[u8],
    ephemeral_key_signature: &[u8],
    _challenge: &[u8],
    private_key: &mut Vec<u8>,
    certificate_chain: &mut Vec<u8>,
) -> Result<Vec<u8>> {
    // Get the attestation public key from the BCC and check that the client's ephemeral key was
    // signed by their attestation key.
    debug!("get their attestation key");
    let chain = bcc::Chain::from_bytes(bcc)?;
    let public_key =
        PublicKey::from_cose_key(&chain.payloads.last().unwrap().subject_public_key.0)?;
    public_key.verify(ephemeral_key_signature, ephemeral_key, &None)?;
    // Generate their "remote attestation" key and encrypt it
    debug!("generate key");
    let key = PKey::generate_ed25519()?;
    // Outputs
    debug!("give them what they want");
    let (ephemeral_key, ciphertext) = respond(ephemeral_key, &key.raw_private_key()?)?;
    *private_key = ciphertext;
    certificate_chain.clear(); // TODO: put something in here
    Ok(ephemeral_key)
}

impl IRkpVmService for RkpVmService {
    fn getRemoteAttestationKey(
        &self,
        bcc: &[u8],
        ephemeral_key: &[u8],
        ephemeral_key_signature: &[u8],
        challenge: &[u8],
        private_key: &mut Vec<u8>,
        certificate_chain: &mut Vec<u8>,
    ) -> BinderResult<Vec<u8>> {
        debug!("got a request for a remote attestation key");
        get_remote_attestation_key(
            bcc,
            ephemeral_key,
            ephemeral_key_signature,
            challenge,
            private_key,
            certificate_chain,
        )
        .map_err(|e| new_binder_exception(ExceptionCode::ILLEGAL_STATE, e.to_string()))
    }
}
