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

use android_hardware_security_keymint::aidl::android::hardware::security::keymint::{
    Algorithm::Algorithm, Digest::Digest, EcCurve::EcCurve, KeyParameter::KeyParameter,
    KeyParameterValue::KeyParameterValue, KeyPurpose::KeyPurpose, SecurityLevel::SecurityLevel,
    Tag::Tag,
};
use android_system_keystore2::aidl::android::system::keystore2::{
    Domain::Domain, IKeystoreService::IKeystoreService, KeyDescriptor::KeyDescriptor,
};
use anyhow::{anyhow, Context, Result};
use binder::{get_interface, Status};
use cert_request_validator::bcc;
use coset::{CborSerializable, CoseSign1};
use log::debug;
use openssl::asn1::Asn1Integer;
use openssl::bn::BigNum;
use openssl::nid::Nid;
use openssl::pkey::{PKey, Public};
use openssl::x509::extension::KeyUsage;
use openssl::x509::{X509Name, X509};

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

fn sign_key(vm_key: PKey<Public>, challenge: &[u8]) -> Result<Vec<u8>> {
    // Connect to keystore
    const KEYSTORE_SERVICE_NAME: &str = "android.system.keystore2.IKeystoreService/default";
    let keystore_service = get_interface::<dyn IKeystoreService>(KEYSTORE_SERVICE_NAME)
        .context("No Keystore service")?;
    let security_level = keystore_service
        .getSecurityLevel(SecurityLevel::TRUSTED_ENVIRONMENT)
        .context("Not TEE keystore")?;

    // Generate an attested Ed25519 signing key
    let key_descriptor = KeyDescriptor {
        domain: Domain::SELINUX,
        nspace: 130,
        alias: Some("rkpvm intermediate key".to_string()),
        blob: None,
    };
    let key_parameters = [
        KeyParameter { tag: Tag::PURPOSE, value: KeyParameterValue::KeyPurpose(KeyPurpose::SIGN) },
        KeyParameter { tag: Tag::ALGORITHM, value: KeyParameterValue::Algorithm(Algorithm::EC) },
        KeyParameter {
            tag: Tag::EC_CURVE,
            value: KeyParameterValue::EcCurve(EcCurve::CURVE_25519),
        },
        KeyParameter { tag: Tag::DIGEST, value: KeyParameterValue::Digest(Digest::NONE) },
        KeyParameter { tag: Tag::NO_AUTH_REQUIRED, value: KeyParameterValue::BoolValue(true) },
        KeyParameter {
            tag: Tag::ATTESTATION_CHALLENGE,
            value: KeyParameterValue::Blob(challenge.to_vec()),
        },
        // TODO: Device IDs would be added here e.g. Tag::ATTESTATION_ID_BRAND. Not for VMs?
    ];
    let attestation_key = None;
    let flags = 0;
    let entropy = [];
    let key_metadata = security_level
        .generateKey(&key_descriptor, attestation_key, &key_parameters, flags, &entropy)
        .context("Generating key failed")?;

    // Make a certificate
    // TODO(b/239549209): replace with x509_cert library, same as rust keymint
    debug!("issuer");
    let mut issuer = X509Name::builder()?;
    issuer.append_entry_by_nid(Nid::COUNTRYNAME, "US")?;
    issuer.append_entry_by_nid(Nid::STATEORPROVINCENAME, "California")?;
    issuer.append_entry_by_nid(Nid::ORGANIZATIONNAME, "Google, Inc.")?;
    issuer.append_entry_by_nid(Nid::ORGANIZATIONALUNITNAME, "Android")?;
    issuer.append_entry_by_nid(Nid::COMMONNAME, "Android Virtualization Framework")?;
    let issuer = issuer.build();

    debug!("subject");
    let mut subject = X509Name::builder()?;
    subject.append_entry_by_nid(Nid::COMMONNAME, "Android Protected Virtual Machine")?;
    let subject = subject.build();

    debug!("cert");
    // TODO: from_der doesn't tell how big it is
    let cert = X509::from_der(key_metadata.certificate.as_ref().unwrap())?;
    let mut builder = X509::builder()?;
    builder.set_version(2)?;
    builder.set_serial_number(Asn1Integer::from_bn(BigNum::from_u32(1)?.as_ref())?.as_ref())?;
    builder.set_issuer_name(&issuer)?;
    builder.set_subject_name(&subject)?;
    builder.set_not_before(cert.not_before())?;
    builder.set_not_after(cert.not_after())?;
    builder.set_pubkey(&vm_key)?;
    builder.append_extension(KeyUsage::new().digital_signature().build()?)?;
    // TODO: add extension e.g. 1.3.6.1.4.1.11129.2.1.17
    // TODO: how to DER encode a custom structure?
    // TODO: openssl missing a bunch of ASN
    //       see system/keymaster/km_openssl/attestation_record.cpp
    //let oid = Asn1Object::from_str("1.3.6.1.4.1.11129.2.1.17")?;
    //let attest_str = ASN1_OCTET_STRING_new
    //    ASN1_OCTET_STRING_set
    //X509_EXTENSION_create_by_OBJ(&oid, &attest_str)?;

    // HACK: temporarily sign with a random key as a KM key can't used directly
    let fake_key = PKey::generate_ed25519()?;
    builder.sign_without_digest(&fake_key)?;
    let cert = builder.build().to_der()?;

    /*
    // Hackily extract the section to sign
    let to_sign = &cert[4..cert.len() - 74];

    // Sign the key
    debug!("sign");
    let operation_parameters = [
        KeyParameter { tag: Tag::PURPOSE, value: KeyParameterValue::KeyPurpose(KeyPurpose::SIGN) },
        KeyParameter { tag: Tag::ALGORITHM, value: KeyParameterValue::Algorithm(Algorithm::EC) },
        KeyParameter {
            tag: Tag::EC_CURVE,
            value: KeyParameterValue::EcCurve(EcCurve::CURVE_25519),
        },
        KeyParameter { tag: Tag::DIGEST, value: KeyParameterValue::Digest(Digest::NONE) },
    ];
    let forced = false;
    let response = security_level
        .createOperation(&key_metadata.key, &operation_parameters, forced)
        .context("Creating operation failed")?;
    let operation = response.iOperation.ok_or_else(|| anyhow!("No operation created"))?;
    let signature = operation.finish(Some(to_sign), None).context("Signing failed")?;
    debug!("replace");

    // TODO: this works sometimes, depeding on the BIT STRING encoding :/
    let signature = signature.unwrap();
    let cert_len = cert.len();
    cert[cert_len - signature.len()..].copy_from_slice(&signature);
    */

    debug!("dump");
    let mut certificate_chain = cert.to_vec();
    certificate_chain.extend_from_slice(&key_metadata.certificate.unwrap());
    certificate_chain.extend_from_slice(&key_metadata.certificateChain.unwrap());

    // TODO: these could leak but it's only a hack
//    operation.abort().unwrap_or_default();
    keystore_service.deleteKey(&key_metadata.key).context("Deleting key")?;
    Ok(certificate_chain)
}

fn get_remotely_attested_certificate(
    dice_cert_chain: &[u8],
    key_to_sign: &[u8],
    challenge: &[u8],
) -> Result<Vec<u8>> {
    debug!("get their attestation key");

    // Check the VM's DICE chain.
    let chain = bcc::Chain::from_bytes(dice_cert_chain)?;
    // TODO: make sure it's from a VM

    // Check that the same VM signed the request.
    let sign1 = CoseSign1::from_slice(key_to_sign).map_err(|ce| anyhow!("Parsing COSE_Sign1: {:?}", ce))?;
    sign1.verify_signature(&[], |s, m| chain.leaf().subject_public_key.verify(s, m)).context("Verifying COSE_Sign1")?;

    // Extract the public key to sign.
    let payload = sign1.payload.as_ref().ok_or_else(|| anyhow!("Missing key to sign"))?;
    let public_key = PKey::public_key_from_der(payload).context("Parsing public key")?;

    // Generate the "remote attestation" certificates
    // Cheat remote attestation with keystore
    debug!("sign key");
    let certificate_chain = sign_key(public_key, challenge).context("Sign key")?;
    Ok(certificate_chain)
}

impl IRkpVmService for RkpVmService {
    fn getRemotelyAttestedCertificate(
        &self,
        dice_cert_chain: &[u8],
        key_to_sign: &[u8],
        challenge: &[u8],
    ) -> BinderResult<Vec<u8>> {
        debug!("got a request for a remote attestation key");
        get_remotely_attested_certificate(
            dice_cert_chain,
            key_to_sign,
            challenge,
        )
        .map_err(|e| Status::new_exception_str(ExceptionCode::ILLEGAL_STATE, Some(e.to_string())))
    }
}
