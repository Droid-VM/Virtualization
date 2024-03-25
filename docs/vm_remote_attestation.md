# VM Remote Attestation

## Introduction

Ensuring the authenticity of a VM is crucial as it guarantees that the VM is
trusted and has not been compromised. VM remote attestation allows a *protected
VM* (pVM) to prove to a third party that:

-   All the components of the VM, including the firmware, OS, and other
    softwares, are valid.
-   The VM is running on a valid device trusted by the RKP
    ([Remote Key Provisioning](rkp)) backend, such as Google.

[rkp]: https://source.android.com/docs/core/ota/modular-system/remote-key-provisioning

## Design

The process of pVM remote attestation involves the use of a lightweight
intermediate VM known as the [RKP VM](rkpvm). This approach divides the
attestation process into two parts: attesting the RKP VM against the RKP server
and attesting the pVM against the RKP VM.

[rkpvm]: https://android.googlesource.com/platform/packages/modules/Virtualization/+/main/service_vm/README.md

### RKP VM attestation

The RKP VM is recognized and attested by the RKP server, which acts as a trusted
entity responsible for verifying the [DICE chain][open-dice] of the RKP VM. This
verification ensures that the RKP VM is operating on a genuine device.
Additionally, the RKP VM is validated by the pVM Firmware, as part of the
verified boot process.

[open-dice]: https://android.googlesource.com/platform/external/open-dice/+/main/docs/android.md

### pVM attestation

Once the RKP VM is successfully attested, it assumes the role of a trusted
platform to attest pVMs. It leverages its trusted status to validate the
integrity of the DICE chain associated with each pVM. This validation process
verifies that the pVMs are running in the expected VM environment, and certifies
the payload executed within the pVM.

## Output

Once a pVM successfully passes the attestation process, it receives an
RKP-backed certificate chain and an attested private key that is only known to
the pVM. The certificate chain includes a leaf certificate that covers the
attested public key. The leaf certificate has a new extension with the OID
`1.3.6.1.4.1.11129.2.1.29.1`, specifically designed to describe the pVM payload
for the third party to verify.

The extension format is as follows:

```
AttestationExtension ::= SEQUENCE {
    attestationChallenge       OCTET_STRING,
    isVmSecure                 BOOLEAN,
    vmComponents               SEQUENCE OF VmComponent,
}

VmComponent ::= SEQUENCE {
    name               UTF8String,
    securityVersion    INTEGER,
    codeHash           OCTET STRING,
    authorityHash      OCTET STRING,
}
```

In `AttestationExtension`:

-   The `attestationChallenge` field represents a challenge provided by the
    third party to ensure the freshness of the certificate.
-   The `isVmSecure` field indicates whether the attested pVM is secure. It is
    set to true only when all the DICE certificates in the pVM DICE chain are in
    normal mode.
-   The `vmComponents` field contains a list of all the APKs and apexes loaded
    by the pVM.

## API

To request remote attestation of a pVM, the NDK-API
[AVmPayload_requestAttestation(challenge)](api) can be invoked within the pVM
payload.

For detailed information and usage examples, please refer to
[demo app](demo).

[api]: https://android.googlesource.com/platform/packages/modules/Virtualization/+/main/vm_payload/include/vm_payload.h
[demo]: https://android.googlesource.com/platform/packages/modules/Virtualization/+/main/service_vm/demo_apk
