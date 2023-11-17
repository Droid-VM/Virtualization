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

//! Generation of certificates and attestation extensions.

use alloc::vec::Vec;
use core::{result, time::Duration};
use der::{
    asn1::{BitStringRef, GeneralizedTime, UIntRef},
    oid::AssociatedOid,
    Decode, Sequence,
};
use service_vm_comm::RequestProcessingError;
use spki::{AlgorithmIdentifier, ObjectIdentifier, SubjectPublicKeyInfo};
use x509_cert::{
    certificate::{Certificate, TbsCertificate, Version},
    ext::Extension,
    name::RdnSequence,
    time::{Time, Validity},
};

type Result<T> = result::Result<T, RequestProcessingError>;

/// Default certificate serial number of 1.
const DEFAULT_CERT_SERIAL: &[u8] = &[0x01];

/// OID value for PKCS#1 signature with SHA-256 and ECDSA, see RFC 5758 s3.2.
const ECDSA_SHA256_SIGNATURE_OID: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.2");

/// OID value for the protected VM remote attestation extension.
const ATTESTATION_EXTENSION_OID: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.3.6.1.4.1.11129.2.1.29");

/// Validity time should be encoded differently before and after the `MAX_UTC_TIME`
/// as required in RFC5280 s4.1.2.5.
const MAX_UTC_TIME: Duration = Duration::from_secs(2524608000); // 2050-01-01T00:00:00Z

/// Current version of the attestation extension.
const ATTESTATION_VERSION: i32 = 1;

/// Attestation extension contents
/// ```asn1
/// AttestationDescription ::= SEQUENCE {
///     attestationVersion         INTEGER, # Value 1
///     attestationChallenge       OCTET_STRING,
/// }
/// ```
/// TODO(b/312448064): Add VM payload information to the extension.
#[derive(Debug, Clone, Sequence)]
pub(crate) struct AttestationExtension<'a> {
    attestation_version: i32,
    #[asn1(type = "OCTET STRING")]
    attestation_challenge: &'a [u8],
}

impl<'a> AssociatedOid for AttestationExtension<'a> {
    const OID: ObjectIdentifier = ATTESTATION_EXTENSION_OID;
}

impl<'a> AttestationExtension<'a> {
    pub(crate) fn new(challenge: &'a [u8]) -> Self {
        Self { attestation_version: ATTESTATION_VERSION, attestation_challenge: challenge }
    }
}

/// Builds an X.509 `Certificate` as defined in RFC 5280 Section 4.1:
///
/// Certificate  ::=  SEQUENCE  {
///   tbsCertificate       TBSCertificate,
///   signatureAlgorithm   AlgorithmIdentifier,
///   signature            BIT STRING
/// }
pub(crate) fn build_certificate<'a>(
    tbs_cert: TbsCertificate<'a>,
    signature: &'a [u8],
) -> Result<Certificate<'a>> {
    Ok(Certificate {
        signature_algorithm: tbs_cert.signature,
        tbs_certificate: tbs_cert,
        signature: BitStringRef::new(0, signature)?,
    })
}

/// Builds an X.509 `TbsCertificate` as defined in RFC 5280 Section 4.1:
///
/// TBSCertificate  ::=  SEQUENCE  {
///   version         [0]  EXPLICIT Version DEFAULT v1,
///   serialNumber         CertificateSerialNumber,
///   signature            AlgorithmIdentifier,
///   issuer               Name,
///   validity             Validity,
///   subject              Name,
///   subjectPublicKeyInfo SubjectPublicKeyInfo,
///   issuerUniqueID  [1]  IMPLICIT UniqueIdentifier OPTIONAL,
///                        -- If present, version MUST be v2 or v3
///   subjectUniqueID [2]  IMPLICIT UniqueIdentifier OPTIONAL,
///                        -- If present, version MUST be v2 or v3
///   extensions      [3]  Extensions OPTIONAL
///                        -- If present, version MUST be v3 --
/// }
pub(crate) fn build_tbs_certificate<'a>(
    subject_public_key_info: &'a [u8],
    attestation_ext: &'a [u8],
) -> Result<TbsCertificate<'a>> {
    // TODO(b/309441500): Assign a correct cert serial number.
    let cert_serial = DEFAULT_CERT_SERIAL;
    let signature = AlgorithmIdentifier { oid: ECDSA_SHA256_SIGNATURE_OID, parameters: None };
    // TODO(b/311359366): Assign the correct validity period.
    let not_before = Time::GeneralTime(GeneralizedTime::from_unix_duration(MAX_UTC_TIME)?);
    let not_after = Time::GeneralTime(GeneralizedTime::from_unix_duration(MAX_UTC_TIME)?);

    let mut extensions = Vec::with_capacity(1);
    let attest_ext = Extension {
        extn_id: AttestationExtension::OID,
        critical: false,
        extn_value: attestation_ext,
    };
    extensions.push(attest_ext);

    let subject_public_key_info = SubjectPublicKeyInfo::from_der(subject_public_key_info)?;
    Ok(TbsCertificate {
        version: Version::V3,
        serial_number: UIntRef::new(cert_serial)?,
        signature,
        // TODO(b/309441500): Add correct issuer.
        issuer: RdnSequence::default(),
        validity: Validity { not_before, not_after },
        // TODO(b/309441500): Add correct subject.
        subject: RdnSequence::default(),
        subject_public_key_info,
        issuer_unique_id: None,
        subject_unique_id: None,
        extensions: Some(extensions),
    })
}
