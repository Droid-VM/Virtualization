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

//! Implementation of the AIDL interface `IVmPayloadService`.

use android_system_virtualization_payload::aidl::android::system::virtualization::payload::{
    AttestationResult::AttestationResult,
    IVmPayloadService::{BnVmPayloadService, IVmPayloadService, VM_PAYLOAD_SERVICE_SOCKET_NAME}
};
use android_system_virtualmachineservice::aidl::android::system::virtualmachineservice::IVirtualMachineService::IVirtualMachineService;
use anyhow::{anyhow, Context, Result};
use avflog::LogResult;
use binder::{Interface, BinderFeatures, ExceptionCode, Strong, IntoBinderResult};
use diced_open_dice::{DiceArtifacts, derive_cdi_leaf_priv, PrivateKey, sign};
use log::info;
use rpcbinder::RpcServer;
use coset::{iana, AsCborValue, CoseSign, CoseSignBuilder, HeaderBuilder, CborSerializable, CoseSignatureBuilder};
use service_vm_comm::CsrPayload;
use std::os::unix::io::OwnedFd;
use crate::vm_secret::{VmSecret};

/// Implementation of `IVmPayloadService`.
struct VmPayloadService {
    allow_restricted_apis: bool,
    virtual_machine_service: Strong<dyn IVirtualMachineService>,
    secret: VmSecret,
}

impl IVmPayloadService for VmPayloadService {
    fn notifyPayloadReady(&self) -> binder::Result<()> {
        self.virtual_machine_service.notifyPayloadReady()
    }

    fn getVmInstanceSecret(&self, identifier: &[u8], size: i32) -> binder::Result<Vec<u8>> {
        if !(0..=32).contains(&size) {
            return Err(anyhow!("size {size} not in range (0..=32)"))
                .or_binder_exception(ExceptionCode::ILLEGAL_ARGUMENT);
        }
        let mut instance_secret = vec![0; size.try_into().unwrap()];
        self.secret
            .derive_payload_sealing_key(identifier, &mut instance_secret)
            .context("Failed to derive VM instance secret")
            .with_log()
            .or_service_specific_exception(-1)?;
        Ok(instance_secret)
    }

    fn getDiceAttestationChain(&self) -> binder::Result<Vec<u8>> {
        self.check_restricted_apis_allowed()?;
        if let Some(bcc) = self.secret.dice().bcc() {
            Ok(bcc.to_vec())
        } else {
            Err(anyhow!("bcc is none")).or_binder_exception(ExceptionCode::ILLEGAL_STATE)
        }
    }

    fn getDiceAttestationCdi(&self) -> binder::Result<Vec<u8>> {
        self.check_restricted_apis_allowed()?;
        Ok(self.secret.dice().cdi_attest().to_vec())
    }

    fn requestAttestation(&self, challenge: &[u8]) -> binder::Result<AttestationResult> {
        self.check_restricted_apis_allowed()?;
        // TODO(b/303807447): Generate the key pair here.
        let public_key = Vec::new();
        let private_key = Vec::new();
        let csr = build_csr(challenge, &public_key, self.secret.dice())
            .context("Failed to build CSR")
            .or_binder_exception(ExceptionCode::ILLEGAL_STATE)?;
        // TODO(b/293871876): Rename the API to requestAttestation.
        let cert_chain = self.virtual_machine_service.requestCertificate(&csr)?;
        Ok(AttestationResult { privateKey: private_key, certificateChain: cert_chain })
    }
}

fn build_csr(
    challenge: &[u8],
    public_key: &[u8],
    dice_artifacts: &dyn DiceArtifacts,
) -> Result<Vec<u8>> {
    let dice_cert_chain = dice_artifacts.bcc().ok_or(anyhow!("bcc is none"))?;
    let csr_payload = CsrPayload {
        challenge: challenge.to_vec(),
        public_key: public_key.to_vec(),
        dice_cert_chain: dice_cert_chain.to_vec(),
    };
    let csr_payload = cbor_util::serialize(&csr_payload)
        .map_err(|e| anyhow!("Failed to serialize Payload: {e}"))?;
    let cdi_leaf_priv = derive_cdi_leaf_priv(dice_artifacts)?;
    let signed_data = build_signed_data(csr_payload, &cdi_leaf_priv)?
        .to_cbor_value()
        .map_err(|e| anyhow!("Failed to serialize signed data to CBOR: {e}"))?;
    signed_data.to_vec().map_err(|e| anyhow!("Failed to serialize CSR: {e}"))
}

fn build_signed_data(payload: Vec<u8>, cdi_leaf_priv: &PrivateKey) -> Result<CoseSign> {
    let signing_algorithm = iana::Algorithm::EdDSA;
    let protected = HeaderBuilder::new().algorithm(signing_algorithm).build();
    let aad = &[];
    let cdi_leaf_sig = CoseSignatureBuilder::new().protected(protected.clone()).build();
    // TODO(b/303807447): Add another signature with the generated private key.
    let signed_data = CoseSignBuilder::new()
        .protected(protected)
        .payload(payload)
        .try_add_created_signature(cdi_leaf_sig, aad, |message| {
            sign(message, cdi_leaf_priv.as_array()).map(|v| v.to_vec())
        })?
        .build();
    Ok(signed_data)
}

impl Interface for VmPayloadService {}

impl VmPayloadService {
    /// Creates a new `VmPayloadService` instance from the `IVirtualMachineService` reference.
    fn new(
        allow_restricted_apis: bool,
        vm_service: Strong<dyn IVirtualMachineService>,
        secret: VmSecret,
    ) -> VmPayloadService {
        Self { allow_restricted_apis, virtual_machine_service: vm_service, secret }
    }

    fn check_restricted_apis_allowed(&self) -> binder::Result<()> {
        if self.allow_restricted_apis {
            Ok(())
        } else {
            Err(anyhow!("Use of restricted APIs is not allowed"))
                .with_log()
                .or_binder_exception(ExceptionCode::SECURITY)
        }
    }
}

/// Registers the `IVmPayloadService` service.
pub(crate) fn register_vm_payload_service(
    allow_restricted_apis: bool,
    vm_service: Strong<dyn IVirtualMachineService>,
    secret: VmSecret,
    vm_payload_service_fd: OwnedFd,
) -> Result<()> {
    let vm_payload_binder = BnVmPayloadService::new_binder(
        VmPayloadService::new(allow_restricted_apis, vm_service, secret),
        BinderFeatures::default(),
    );

    let server = RpcServer::new_bound_socket(vm_payload_binder.as_binder(), vm_payload_service_fd)?;
    info!("The RPC server '{}' is running.", VM_PAYLOAD_SERVICE_SOCKET_NAME);

    // Move server reference into a background thread and run it forever.
    std::thread::spawn(move || {
        server.join();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwtrust::{dice, session::Session};

    /// The following data is generated randomly with urandom.
    const CHALLENGE: [u8; 16] = [
        0xb3, 0x66, 0xfa, 0x72, 0x92, 0x32, 0x2c, 0xd4, 0x99, 0xcb, 0x00, 0x1f, 0x0e, 0xe0, 0xc7,
        0x41,
    ];
    const PUBLIC_KEY1: [u8; 32] = [
        0x04, 0xEA, 0x7A, 0x29, 0x31, 0xCD, 0x43, 0xE3, 0x09, 0xD2, 0x22, 0x35, 0x75, 0x6B, 0x22,
        0x5C, 0x08, 0x01, 0x17, 0xCF, 0xB0, 0x6D, 0xE3, 0x16, 0x95, 0xA5, 0xD3, 0x55, 0x33, 0x38,
        0xB9, 0xC2,
    ];

    #[test]
    fn csr_has_correct_format() -> Result<()> {
        let dice_artifacts = diced_sample_inputs::make_sample_bcc_and_cdis()?;

        let csr_vec = build_csr(&CHALLENGE, &PUBLIC_KEY1, &dice_artifacts)?;

        let cose_sign = CoseSign::from_slice(&csr_vec).unwrap();
        let csr_payload = cose_sign.payload.clone().unwrap();
        let csr_payload: CsrPayload = cbor_util::deserialize(&csr_payload).unwrap();

        // Checks the first signature is signed with CDI_Leaf_Priv.
        let session = Session::default();
        let chain = dice::Chain::from_cbor(&session, &csr_payload.dice_cert_chain)?;
        let public_key = chain.leaf().subject_public_key();
        let aad = &[];
        cose_sign
            .verify_signature(0, aad, |signature, message| public_key.verify(signature, message))?;

        let expected_csr_payload = CsrPayload {
            challenge: CHALLENGE.to_vec(),
            public_key: PUBLIC_KEY1.to_vec(),
            dice_cert_chain: dice_artifacts.bcc().unwrap().to_vec(),
        };
        assert_eq!(expected_csr_payload, csr_payload);
        Ok(())
    }
}
