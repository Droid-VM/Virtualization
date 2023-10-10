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

use android_system_virtualization_payload::aidl::android::system::virtualization::payload::IVmPayloadService::{
    BnVmPayloadService, IVmPayloadService, VM_PAYLOAD_SERVICE_SOCKET_NAME};
use android_system_virtualmachineservice::aidl::android::system::virtualmachineservice::IVirtualMachineService::IVirtualMachineService;
use anyhow::{anyhow, Context, Result};
use avflog::LogResult;
use binder::{Interface, BinderFeatures, ExceptionCode, Strong, IntoBinderResult};
use diced_open_dice::{DiceArtifacts, keypair_from_seed, PrivateKey, sign};
use log::info;
use rpcbinder::RpcServer;
use coset::{iana, AsCborValue, CoseSign1, CoseSign1Builder, HeaderBuilder, CborSerializable};
use std::os::unix::io::OwnedFd;
use ciborium::{cbor, value::Value};
use crate::vm_secret::{VmSecret};

/// Implementation of `IVmPayloadService`.
struct VmPayloadService {
    allow_restricted_apis: bool,
    virtual_machine_service: Strong<dyn IVirtualMachineService>,
    secret: VmSecret,
}

const REMOTE_ATTESTATION_CSR_SCHEMA_V1: u8 = 1;

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

    fn requestCertificate(&self, public_key: &[u8]) -> binder::Result<Vec<u8>> {
        self.check_restricted_apis_allowed()?;
        let csr = build_csr(public_key, self.secret.dice())
            .context("Failed to build CSR")
            .or_binder_exception(ExceptionCode::ILLEGAL_STATE)?;
        self.virtual_machine_service.requestCertificate(&csr)
    }
}

fn build_csr(public_key: &[u8], dice_artifacts: &dyn DiceArtifacts) -> Result<Vec<u8>> {
    let dice_cert_chain = dice_artifacts.bcc().ok_or(anyhow!("bcc is none"))?;
    let dice_cert_chain = Value::from_slice(dice_cert_chain)
        .map_err(|e| anyhow!("Failed to deserialize from CBOR: {e}"))?;

    // TODO(b/304449735): Add challenge from client server to remote attestation CSR
    // TODO(b/304449739): Add Apk/Apexes loaded in microdroid to remote attestation CSR
    let signed_data_payload = cbor!([Value::Bytes(public_key.to_vec())])?;
    let signed_data_payload =
        signed_data_payload.to_vec().map_err(|e| anyhow!("Failed to serialize Payload: {e}"))?;
    let cdi_leaf_priv =
        derive_cdi_leaf_priv(dice_artifacts).context("Failed to derive the CDI_Leaf_Priv")?;
    let signed_data = build_signed_data(signed_data_payload, &cdi_leaf_priv)?
        .to_cbor_value()
        .map_err(|e| anyhow!("Failed to serialize signed data to CBOR: {e}"))?;

    let csr = cbor!([
        Value::Integer(REMOTE_ATTESTATION_CSR_SCHEMA_V1.into()),
        dice_cert_chain,
        signed_data,
    ])?;
    csr.to_vec().map_err(|e| anyhow!("Failed to serialize CSR: {e}"))
}

fn build_signed_data(payload: Vec<u8>, private_key: &PrivateKey) -> Result<CoseSign1> {
    let signing_algorithm = iana::Algorithm::EdDSA;
    let protected = HeaderBuilder::new().algorithm(signing_algorithm).build();
    let aad = &[];
    let signed_data = CoseSign1Builder::new()
        .protected(protected)
        .payload(payload)
        .try_create_signature(aad, |message| sign_message(message, private_key))?
        .build();
    Ok(signed_data)
}

fn sign_message(message: &[u8], private_key: &PrivateKey) -> Result<Vec<u8>> {
    let signature = sign(message, private_key.as_array()).context("Failed to sign the CSR")?;
    Ok(signature.to_vec())
}

fn derive_cdi_leaf_priv(dice_artifacts: &dyn DiceArtifacts) -> diced_open_dice::Result<PrivateKey> {
    let (_, private_key) = keypair_from_seed(dice_artifacts.cdi_attest())?;
    Ok(private_key)
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
    use anyhow::bail;

    /// The following data is generated randomly with urandom.
    const PUBLIC_KEY1: [u8; 32] = [
        0x04, 0xEA, 0x7A, 0x29, 0x31, 0xCD, 0x43, 0xE3, 0x09, 0xD2, 0x22, 0x35, 0x75, 0x6B, 0x22,
        0x5C, 0x08, 0x01, 0x17, 0xCF, 0xB0, 0x6D, 0xE3, 0x16, 0x95, 0xA5, 0xD3, 0x55, 0x33, 0x38,
        0xB9, 0xC2,
    ];

    #[test]
    fn csr_format_is_correct() -> Result<()> {
        let dice_artifacts = diced_sample_inputs::make_sample_bcc_and_cdis()?;
        let csr_vec = build_csr(&PUBLIC_KEY1, &dice_artifacts)?;

        let Value::Array(csr) = Value::from_slice(&csr_vec).unwrap() else {
            bail!("Wrong CSR format: {csr_vec:?}")
        };
        assert_eq!(3, csr.len());
        assert_eq!(Value::Integer(REMOTE_ATTESTATION_CSR_SCHEMA_V1.into()), csr[0]);

        let dice_cert_chain = Value::from_slice(dice_artifacts.bcc().unwrap()).unwrap();
        assert_eq!(dice_cert_chain, csr[1]);

        let cose_sign1 = CoseSign1::from_cbor_value(csr[2].clone()).unwrap();
        let expected_payload = cbor!([Value::Bytes(PUBLIC_KEY1.to_vec())])?.to_vec().unwrap();
        assert_eq!(Some(expected_payload), cose_sign1.payload);
        // TODO(b/300625792): Verify the signature with CDI_Leaf_Pub from the dice chain
        Ok(())
    }
}
