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
use binder::get_interface;
use binder_common::new_binder_exception;
use cert_request_validator::bcc;
use log::debug;
use openssl::asn1::Asn1Integer;
use openssl::bn::BigNum;
use openssl::nid::Nid;
use openssl::pkey::PKey;
use openssl::x509::extention::KeyPurpose;
use openssl::x509::{X509Name, X509};
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

fn generate_key(challenge: &[u8], certificate_chain: &mut Vec<u8>) -> Result<Vec<u8>> {
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

    // Generate the key for the VM
    let key = PKey::generate_ed25519()?;

    // Make a certificate
    debug!("issuer");
    let mut issuer = X509Name::builder()?;
    issuer.append_entry_by_nid(Nid::COUNTRYNAME, "US")?;
    issuer.append_entry_by_nid(Nid::STATEORPROVINCENAME, "California")?;
    issuer.append_entry_by_nid(Nid::ORGANIZATIONNAME, "Google, Inc.")?;
    issuer.append_entry_by_nid(Nid::ORGANIZATIONALUNITNAME, "Android")?;
    issuer.append_entry_by_nid(Nid::COMMONNAME, "Android Keystore Key")?;
    let issuer = issuer.build();

    debug!("subject");
    let mut subject = X509Name::builder()?;
    subject.append_entry_by_nid(Nid::COMMONNAME, "Android Protected Virtual Machine Key")?;
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
    builder.set_pubkey(&key)?;
    builder.add_extension(KeyUsage::new().digital_signature().build()?);
    // TODO: add extension e.g. 1.3.6.1.4.1.11129.2.1.17
    // TODO: how to DER encode a custom structure?
    // TODO: openssl missing a bunch of ASN
    //       see system/keymaster/km_openssl/attestation_record.cpp
    //let oid = Asn1Object::from_str("1.3.6.1.4.1.11129.2.1.17")?;
    //let attest_str = ASN1_OCTET_STRING_new
    //    ASN1_OCTET_STRING_set
    //X509_EXTENSION_create_by_OBJ(&oid, &attest_str)?;
    builder.sign_without_digest(&key)?;
    let mut cert = builder.build().to_der()?;

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

    debug!("dump");
    *certificate_chain = cert.to_vec();
    certificate_chain.extend_from_slice(&key_metadata.certificate.unwrap());
    certificate_chain.extend_from_slice(&key_metadata.certificateChain.unwrap());

    // TODO: these could leak but it's only a hack
    operation.abort().unwrap_or_default();
    keystore_service.deleteKey(&key_metadata.key).context("Deleting key")?;
    key.raw_private_key().context("Raw private key")
}

fn get_remote_attestation_key(
    bcc: &[u8],
    ephemeral_key: &[u8],
    ephemeral_key_signature: &[u8],
    challenge: &[u8],
    private_key: &mut Vec<u8>,
    certificate_chain: &mut Vec<u8>,
) -> Result<Vec<u8>> {
    // Get the attestation public key from the BCC and check that the client's ephemeral key was
    // signed by their attestation key.
    debug!("get their attestation key");
    let chain = bcc::Chain::from_bytes(bcc)?;
    chain.leaf_public_key().verify(ephemeral_key_signature, ephemeral_key)?;
    // TODO: inspect the chain root and certs
    // Generate their "remote attestation" key and encrypt it
    // Cheat remote attestation with keystore
    debug!("generate key");
    let raw_private_key = generate_key(challenge, certificate_chain)?;
    // Outputs
    debug!("give them what they want");
    let (ephemeral_key, ciphertext) = respond(ephemeral_key, &raw_private_key)?;
    *private_key = ciphertext;
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
