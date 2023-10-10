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
    BnVmPayloadService, IVmPayloadService, VM_PAYLOAD_SERVICE_SOCKET_NAME, AttestationResult::AttestationResult
};
use android_system_virtualmachineservice::aidl::android::system::virtualmachineservice::IVirtualMachineService::IVirtualMachineService;
use anyhow::{anyhow, Context, Result};
use avflog::LogResult;
use binder::{Interface, BinderFeatures, ExceptionCode, Strong, IntoBinderResult};
use diced_open_dice::{DiceArtifacts, derive_cdi_leaf_priv, PrivateKey, sign};
use log::info;
use rpcbinder::RpcServer;
use coset::{iana, CoseSign, CoseSignBuilder, HeaderBuilder, CborSerializable, CoseSignatureBuilder, CoseKey, CoseKeyBuilder};
use service_vm_comm::{Csr, CsrPayload};
use std::os::unix::io::OwnedFd;
use openssl::{
    ec::{EcGroup, EcKey, EcKeyRef}, nid::Nid, bn::{BigNumContext, BigNum},
    ecdsa::EcdsaSig, pkey::Private
};
use crate::vm_secret::{VmSecret};

const ATTESTATION_KEY_NID: Nid = Nid::X9_62_PRIME256V1; // NIST P-256 curve
const ATTESTATION_KEY_ALGO: iana::Algorithm = iana::Algorithm::ES256;
const ATTESTATION_KEY_CURVE: iana::EllipticCurve = iana::EllipticCurve::P_256;
const P256_AFFINE_COORDINATE_SIZE: i32 = 32;
const P256_PRIVATE_KEY_SIZE: i32 = 32;

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
        let (private_key, csr) = generate_attestation_key_and_csr(challenge, self.secret.dice())
            .context("Failed to generate attestation key and CSR")
            .or_binder_exception(ExceptionCode::ILLEGAL_STATE)?;
        let cert_chain = self.virtual_machine_service.requestAttestation(&csr)?;
        Ok(AttestationResult { privateKey: private_key, certificateChain: cert_chain })
    }
}

fn generate_attestation_key_and_csr(
    challenge: &[u8],
    dice_artifacts: &dyn DiceArtifacts,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let group = EcGroup::from_curve_name(ATTESTATION_KEY_NID)?;
    let attestation_key = EcKey::generate(&group)?;

    let csr = build_csr(challenge, attestation_key.as_ref(), dice_artifacts)?;
    let csr = csr.into_cbor_vec().map_err(|e| anyhow!("Failed to serialize CSR: {e}"))?;

    let private_key = to_cose_private_key(&attestation_key)?
        .to_vec()
        .map_err(|e| anyhow!("Failed to serialize private key: {e}"))?;
    Ok((private_key, csr))
}

fn build_csr(
    challenge: &[u8],
    attestation_key: &EcKeyRef<Private>,
    dice_artifacts: &dyn DiceArtifacts,
) -> Result<Csr> {
    // Builds CSR Payload to be signed.
    let csr_payload = CsrPayload { challenge: challenge.to_vec() };
    let csr_payload =
        csr_payload.into_cbor_vec().map_err(|e| anyhow!("Failed to serialize CSR Payload: {e}"))?;

    // Builds signed CSR Payload.
    let cdi_leaf_priv = derive_cdi_leaf_priv(dice_artifacts)?;
    let signed_csr_payload = build_signed_data(csr_payload, &cdi_leaf_priv, attestation_key)?
        .to_vec()
        .map_err(|e| anyhow!("Failed to serialize COSE_Sign: {e}"))?;

    // Builds CSR.
    let dice_cert_chain = dice_artifacts.bcc().ok_or(anyhow!("bcc is none"))?.to_vec();
    let public_key = to_cose_public_key(attestation_key)?
        .to_vec()
        .map_err(|e| anyhow!("Failed to serialize public key: {e}"))?;
    Ok(Csr { dice_cert_chain, public_key, signed_csr_payload })
}

fn build_signed_data(
    payload: Vec<u8>,
    cdi_leaf_priv: &PrivateKey,
    attestation_key: &EcKeyRef<Private>,
) -> Result<CoseSign> {
    let signing_algorithm = iana::Algorithm::EdDSA;
    let protected = HeaderBuilder::new().algorithm(signing_algorithm).build();
    let aad = &[];
    let cdi_leaf_sig = CoseSignatureBuilder::new().protected(protected.clone()).build();
    let attestation_key_sig = CoseSignatureBuilder::new().protected(protected.clone()).build();
    let signed_data = CoseSignBuilder::new()
        .protected(protected)
        .payload(payload)
        .try_add_created_signature(cdi_leaf_sig, aad, |message| {
            sign(message, cdi_leaf_priv.as_array()).map(|v| v.to_vec())
        })?
        .try_add_created_signature(attestation_key_sig, aad, |message| {
            ecdsa_sign(message, attestation_key)
        })?
        .build();
    Ok(signed_data)
}

fn ecdsa_sign(message: &[u8], key: &EcKeyRef<Private>) -> Result<Vec<u8>> {
    let sig = EcdsaSig::sign::<Private>(message, key)?;
    Ok(sig.to_der()?)
}

fn get_affine_coordinates(key: &EcKeyRef<Private>) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut ctx = BigNumContext::new()?;
    let mut x = BigNum::new()?;
    let mut y = BigNum::new()?;
    key.public_key().affine_coordinates_gfp(key.group(), &mut x, &mut y, &mut ctx)?;
    let x = x.to_vec_padded(P256_AFFINE_COORDINATE_SIZE)?;
    let y = y.to_vec_padded(P256_AFFINE_COORDINATE_SIZE)?;
    Ok((x, y))
}

fn to_cose_private_key(key: &EcKeyRef<Private>) -> Result<CoseKey> {
    let (x, y) = get_affine_coordinates(key)?;
    let private_key = key.private_key().to_vec_padded(P256_PRIVATE_KEY_SIZE)?;
    Ok(CoseKeyBuilder::new_ec2_priv_key(ATTESTATION_KEY_CURVE, x, y, private_key)
        .algorithm(ATTESTATION_KEY_ALGO)
        .build())
}

fn to_cose_public_key(key: &EcKeyRef<Private>) -> Result<CoseKey> {
    let (x, y) = get_affine_coordinates(key)?;
    Ok(CoseKeyBuilder::new_ec2_pub_key(ATTESTATION_KEY_CURVE, x, y)
        .algorithm(ATTESTATION_KEY_ALGO)
        .build())
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
    use ciborium::Value;
    use coset::{iana::EnumI64, Algorithm, KeyType, Label};
    use hwtrust::{dice, session::Session};
    use openssl::{
        ec::{EcPoint, EcPointRef},
        pkey::Public,
    };

    /// The following data is generated randomly with urandom.
    const CHALLENGE: [u8; 16] = [
        0xb3, 0x66, 0xfa, 0x72, 0x92, 0x32, 0x2c, 0xd4, 0x99, 0xcb, 0x00, 0x1f, 0x0e, 0xe0, 0xc7,
        0x41,
    ];

    #[test]
    fn csr_has_correct_format() -> Result<()> {
        let dice_artifacts = diced_sample_inputs::make_sample_bcc_and_cdis()?;

        let (private_key, csr) = generate_attestation_key_and_csr(&CHALLENGE, &dice_artifacts)?;
        let ec_private_key = to_ec_private_key(&CoseKey::from_slice(&private_key).unwrap())?;
        let csr = Csr::from_cbor_slice(&csr).unwrap();

        // Checks the public key in the CSR corresponds to the private key.
        let attestation_public_key = CoseKey::from_slice(&csr.public_key).unwrap();
        let ec_public_key = to_ec_public_key(&attestation_public_key)?;
        check_public_keys_eq(ec_private_key.public_key(), ec_public_key.public_key())?;

        let cose_sign = CoseSign::from_slice(&csr.signed_csr_payload).unwrap();

        // Checks CSR payload.
        let csr_payload = cose_sign.payload.clone().unwrap();
        let csr_payload: CsrPayload = CsrPayload::from_cbor_slice(&csr_payload).unwrap();
        let expected_csr_payload = CsrPayload { challenge: CHALLENGE.to_vec() };
        assert_eq!(expected_csr_payload, csr_payload);
        let aad = &[];

        // Checks the first signature is signed with CDI_Leaf_Priv.
        let session = Session::default();
        let chain = dice::Chain::from_cbor(&session, &csr.dice_cert_chain)?;
        let public_key = chain.leaf().subject_public_key();
        cose_sign
            .verify_signature(0, aad, |signature, message| public_key.verify(signature, message))?;

        // Checks the second signature is signed with the attestation key.
        cose_sign.verify_signature(1, aad, |signature, message| {
            ecdsa_verify(signature, message, &ec_public_key)
        })?;

        Ok(())
    }

    fn check_public_keys_eq(a: &EcPointRef, b: &EcPointRef) -> Result<()> {
        let group = EcGroup::from_curve_name(ATTESTATION_KEY_NID)?;
        let mut ctx = BigNumContext::new()?;
        if a.eq(&group, b, &mut ctx)? {
            Ok(())
        } else {
            bail!("Public keys are not equal")
        }
    }

    fn ecdsa_verify(
        signature: &[u8],
        message: &[u8],
        ec_public_key: &EcKeyRef<Public>,
    ) -> Result<()> {
        let sig = EcdsaSig::from_der(signature)?;
        if sig.verify(message, ec_public_key)? {
            Ok(())
        } else {
            bail!("Signature does not match")
        }
    }

    fn to_ec_private_key(cose_key: &CoseKey) -> Result<EcKey<Private>> {
        check_ec_key_params(cose_key)?;
        let group = EcGroup::from_curve_name(ATTESTATION_KEY_NID)?;
        let mut public_key = EcPoint::new(&group)?;
        let x = get_label_value_as_bignum(cose_key, Label::Int(iana::Ec2KeyParameter::X.to_i64()))?;
        let y = get_label_value_as_bignum(cose_key, Label::Int(iana::Ec2KeyParameter::Y.to_i64()))?;
        let mut ctx = BigNumContext::new()?;
        public_key.set_affine_coordinates_gfp(&group, &x, &y, &mut ctx)?;

        let private_key =
            get_label_value_as_bignum(cose_key, Label::Int(iana::Ec2KeyParameter::D.to_i64()))?;
        let key = EcKey::from_private_components(&group, &private_key, &public_key)?;
        key.check_key()?;
        Ok(key)
    }

    fn to_ec_public_key(cose_key: &CoseKey) -> Result<EcKey<Public>> {
        check_ec_key_params(cose_key)?;
        let group = EcGroup::from_curve_name(ATTESTATION_KEY_NID)?;
        let x = get_label_value_as_bignum(cose_key, Label::Int(iana::Ec2KeyParameter::X.to_i64()))?;
        let y = get_label_value_as_bignum(cose_key, Label::Int(iana::Ec2KeyParameter::Y.to_i64()))?;
        let key = EcKey::from_public_key_affine_coordinates(&group, &x, &y)?;
        key.check_key()?;
        Ok(key)
    }

    fn check_ec_key_params(cose_key: &CoseKey) -> Result<()> {
        assert_eq!(KeyType::Assigned(iana::KeyType::EC2), cose_key.kty);
        assert_eq!(Some(Algorithm::Assigned(iana::Algorithm::ES256)), cose_key.alg);
        let crv = get_label_value(cose_key, Label::Int(iana::Ec2KeyParameter::Crv.to_i64()))?;
        assert_eq!(&Value::from(iana::EllipticCurve::P_256.to_i64()), crv);
        Ok(())
    }

    fn get_label_value_as_bignum(key: &CoseKey, label: Label) -> Result<BigNum> {
        get_label_value(key, label)?
            .as_bytes()
            .map(|v| BigNum::from_slice(&v[..]).unwrap())
            .ok_or_else(|| anyhow!("Value not a bstr."))
    }

    fn get_label_value(key: &CoseKey, label: Label) -> Result<&Value> {
        Ok(&key
            .params
            .iter()
            .find(|(k, _)| k == &label)
            .ok_or_else(|| anyhow!("Label {:?} not found", label))?
            .1)
    }
}
