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
//!
//! TODO: the logic should all move into an actual VM,

use anyhow::{anyhow, ensure, Context, Result};
use cert_request_validator::bcc;
use coset::{CborSerializable, CoseSign1};
use foreign_types::ForeignType;
use lazy_static::lazy_static;
use log::debug;
use openssl::asn1::{Asn1Integer, Asn1Time};
use openssl::bn::BigNum;
use openssl::nid::Nid;
use openssl::pkey::{PKey, Public};
use openssl::x509::extension::KeyUsage;
use openssl::x509::{X509Extension, X509Name, X509};
use rkpvm_ext_bindgen::{
    avf_extension_details, generate_avf_extension, verified_boot_state_UNVERIFIED,
    vm_payload_details, vm_root_of_trust_details,
};

lazy_static! {
    static ref AVF_EXT_NID: Nid = Nid::create(
        "1.3.6.1.4.1.11129.2.1.29",
        "avfAttestationExt",
        "Android Virtualization Framework Attestation Extension"
    )
    .unwrap_or(Nid::UNDEF);
}

fn avf_extension(challenge: &[u8]) -> Result<X509Extension> {
    ensure!(*AVF_EXT_NID != Nid::UNDEF, "AVF attestation NID not allocated");
    // TODO: marshal all of the details
    let details = avf_extension_details {
        nid: AVF_EXT_NID.as_raw(),
        challenge: challenge.as_ptr(),
        challenge_size: challenge.len(),
        vm_root_of_trust: vm_root_of_trust_details {
            verified_boot_key: std::ptr::null(),
            verified_boot_key_size: 0,
            verified_boot_state: verified_boot_state_UNVERIFIED,
            device_unlocked: true,
            debuggable: true,
        },
        vm_payload: vm_payload_details {
            authority: std::ptr::null(),
            authority_size: 0,
            digest: std::ptr::null(),
            digest_size: 0,
            binary_path: std::ptr::null(),
            binary_path_size: 0,
        },
    };
    // SAFETY: The extension generationg code only uses the details as inputs and does not keep any
    // pointers to the details after returning. If the returned pointer is non-null, the ownership
    // is transferred to the caller.
    let ptr = unsafe { generate_avf_extension(&details) };
    ensure!(!ptr.is_null(), "Failed to make extension");
    // SAFETY: The pointer is an owned allocation that only differs in type due to coming from a
    // different bindgen, both are boringssl X509_Extension pointers. The X509Extension will ensure
    // the pointer is freed when it is no longer in use.
    Ok(unsafe { X509Extension::from_ptr(std::mem::transmute(ptr)) })
}

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
    builder.append_extension(KeyUsage::new().critical().digital_signature().build()?)?;
    builder.append_extension(avf_extension(challenge)?)?;

    // HACK: sign with a random key, not the RKP provisioned key
    let fake_key = PKey::generate_ed25519()?;
    builder.sign_without_digest(&fake_key)?;
    let cert = builder.build().to_der()?;

    debug!("dump");
    // TODO: include the rest of the chain from RKP
    let certificate_chain = cert.to_vec();
    Ok(certificate_chain)
}

pub fn get_remotely_attested_certificate(
    dice_cert_chain: &[u8],
    key_to_sign: &[u8],
    challenge: &[u8],
) -> Result<Vec<u8>> {
    debug!("get their attestation key");

    // Check the VM's DICE chain.
    let chain = bcc::Chain::from_bytes(dice_cert_chain)?;

    // TODO: make sure the chain is from a VM by getting rkpvm's own DICE chain and check that
    // everything is the same up until the certificate for the rkpvm payload (which should be the
    // last certificate in the rkpvm chain). This makes sure the chain begins with:
    //     ROM -> BLs.. -> pvmfw -> ...

    // TODO: make sure the chain describes a microdroid VM by embedding a measurement of microdroid
    // in rkpvm and checking that the embedded measurement is seen in the certificate generated by
    // pvmfw (assuming we've done away with the microdroid bootloader). After this point, we know
    // the chain begins with:
    //     ROM -> BLs.. -> pvmfw -> microdroid -> ...

    //  TODO: make sure the microdroid-generated certificate corresponds to a standard "API VM"
    //  i.e. it's not compos or anything that couldn't be created with the public AVF API. This
    //  will likely require some help from microdroid adding details to the certificate. Now the
    //  chain is known to be:
    //     ROM -> BLs.. -> pvmfw -> microdroid -> payload (AVF API)

    // TODO: make sure there are no certificates following the payload (not 100% necessary) and
    // extract the payload details for inclusion in the remote attestation certificate.

    // Check that the same VM is making the request, by checking the signature of the request.
    let sign1 =
        CoseSign1::from_slice(key_to_sign).map_err(|ce| anyhow!("Parsing COSE_Sign1: {:?}", ce))?;
    sign1
        .verify_signature(&[], |s, m| chain.leaf().subject_public_key.verify(s, m))
        .context("Verifying COSE_Sign1")?;

    // Extract the public key to sign.
    let payload = sign1.payload.as_ref().ok_or_else(|| anyhow!("Missing key to sign"))?;
    let public_key = PKey::public_key_from_der(payload).context("Parsing public key")?;

    // Generate the remote attestation certificates
    // TODO: include the details taked from the VM's DICE chain and sign the certificate with a key
    // obtained via rkpd.
    debug!("sign key");
    let certificate_chain = sign_key(public_key, challenge).context("Sign key")?;
    Ok(certificate_chain)
}
