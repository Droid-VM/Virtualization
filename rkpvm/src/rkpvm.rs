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

use anyhow::{anyhow, Context, Result};
use binder::Status;
use cert_request_validator::bcc;
use coset::{CborSerializable, CoseSign1};
use log::debug;
use openssl::asn1::{Asn1Integer, Asn1Time};
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
    // TODO: get the RKP provisioend key from rkpd

    // Make a certificate
    // TODO(b/239549209): replace with x509_cert library (like keymint) or boringssl in c(++)
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
    let mut builder = X509::builder()?;
    builder.set_version(2)?;
    builder.set_serial_number(Asn1Integer::from_bn(BigNum::from_u32(1)?.as_ref())?.as_ref())?;
    builder.set_issuer_name(&issuer)?;
    builder.set_subject_name(&subject)?;
    // TODO: set to lifetime of the RKP key
    builder.set_not_before(Asn1Time::days_from_now(0)?.as_ref())?;
    builder.set_not_after(Asn1Time::days_from_now(30)?.as_ref())?;
    builder.set_pubkey(&vm_key)?;
    builder.append_extension(KeyUsage::new().digital_signature().build()?)?;
    // TODO: add extension 1.3.6.1.4.1.11129.2.1.x29
    let _ = challenge;
    // TODO: how to DER encode a custom structure?
    // TODO: openssl missing a bunch of ASN
    //       see system/keymaster/km_openssl/attestation_record.cpp
    //let oid = Asn1Object::from_str("1.3.6.1.4.1.11129.2.1.29")?;
    //let attest_str = ASN1_OCTET_STRING_new
    //    ASN1_OCTET_STRING_set
    //X509_EXTENSION_create_by_OBJ(&oid, &attest_str)?;

    // HACK: sign with a random key, not the RKP provisioned key
    let fake_key = PKey::generate_ed25519()?;
    builder.sign_without_digest(&fake_key)?;
    let cert = builder.build().to_der()?;

    debug!("dump");
    // TODO: include the rest of the chain from RKP
    let certificate_chain = cert.to_vec();
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
    let sign1 =
        CoseSign1::from_slice(key_to_sign).map_err(|ce| anyhow!("Parsing COSE_Sign1: {:?}", ce))?;
    sign1
        .verify_signature(&[], |s, m| chain.leaf().subject_public_key.verify(s, m))
        .context("Verifying COSE_Sign1")?;

    // Extract the public key to sign.
    let payload = sign1.payload.as_ref().ok_or_else(|| anyhow!("Missing key to sign"))?;
    let public_key = PKey::public_key_from_der(payload).context("Parsing public key")?;

    // Generate the "remote attestation" certificates
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
        get_remotely_attested_certificate(dice_cert_chain, key_to_sign, challenge).map_err(|e| {
            Status::new_exception_str(ExceptionCode::ILLEGAL_STATE, Some(e.to_string()))
        })
    }
}
