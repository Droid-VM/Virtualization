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

//! Handles the RKP (Remote Key Provisioning) VM and host communication.
//! The RKP VM will be recognized and attested by the RKP server periodically and
//! serves as a trusted platform to attest a client VM.

use android_hardware_security_rkp::aidl::android::hardware::security::keymint::MacedPublicKey::MacedPublicKey;
use anyhow::{bail, Context, Result};
use service_vm_comm::{Csr, GenerateCertificateRequestParams, Request, Response, VmDescriptor};
use service_vm_manager::ServiceVm;

pub(crate) fn request_certificate(_csr: &[u8]) -> Result<Vec<u8>> {
    let mut vm = ServiceVm::start()?;

    // TODO(b/278717513): Fill the following fields with real data
    let csr = Csr {
        public_key: vec![],
        vm_descriptor: VmDescriptor { dice_cert_chain: vec![] },
        vm_descriptor_signature: vec![],
    };
    let request = Request::RequestCertificate(csr);
    match vm.process_request(request).context("Failed to process request")? {
        Response::RequestCertificate(cert) => Ok(cert),
        _ => bail!("Incorrect response type"),
    }
}

pub(crate) fn generate_ecdsa_p256_key_pair() -> Result<Response> {
    let mut vm = ServiceVm::start()?;
    let request = Request::GenerateEcdsaP256KeyPair;
    vm.process_request(request).context("Failed to process request")
}

pub(crate) fn generate_certificate_request(
    keys_to_sign: &[MacedPublicKey],
    challenge: &[u8],
) -> Result<Response> {
    let params = GenerateCertificateRequestParams {
        keys_to_sign: keys_to_sign.iter().map(|v| v.macedKey.to_vec()).collect(),
        challenge: challenge.to_vec(),
    };
    let request = Request::GenerateCertificateRequest(params);

    let mut vm = ServiceVm::start()?;
    vm.process_request(request).context("Failed to process request")
}
