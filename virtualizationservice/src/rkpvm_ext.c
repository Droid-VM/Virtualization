#include "rkpvm_ext.h"

#include <openssl/asn1t.h>
#include <openssl/mem.h>
#include <openssl/x509v3.h>

typedef struct {
    ASN1_OCTET_STRING *verified_boot_key;
    ASN1_ENUMERATED *verified_boot_state;
    ASN1_BOOLEAN device_unlocked;
    ASN1_BOOLEAN debuggable; /* TODO: move to payload?*/
} VM_ROOT_OF_TRUST;

ASN1_SEQUENCE(VM_ROOT_OF_TRUST) = {
        ASN1_SIMPLE(VM_ROOT_OF_TRUST, verified_boot_key, ASN1_OCTET_STRING),
        ASN1_SIMPLE(VM_ROOT_OF_TRUST, verified_boot_state, ASN1_ENUMERATED),
        ASN1_SIMPLE(VM_ROOT_OF_TRUST, device_unlocked, ASN1_BOOLEAN),
        ASN1_SIMPLE(VM_ROOT_OF_TRUST, debuggable, ASN1_BOOLEAN),
} ASN1_SEQUENCE_END(VM_ROOT_OF_TRUST);

DECLARE_ASN1_FUNCTIONS(VM_ROOT_OF_TRUST);
IMPLEMENT_ASN1_FUNCTIONS(VM_ROOT_OF_TRUST);

typedef struct {
    ASN1_OCTET_STRING *authority;
    ASN1_OCTET_STRING *digest;
    ASN1_OCTET_STRING *binary_path;
} VM_PAYLOAD;

ASN1_SEQUENCE(VM_PAYLOAD) = {
        ASN1_SIMPLE(VM_PAYLOAD, authority, ASN1_OCTET_STRING),
        ASN1_SIMPLE(VM_PAYLOAD, digest, ASN1_OCTET_STRING),
        ASN1_SIMPLE(VM_PAYLOAD, binary_path, ASN1_OCTET_STRING),
} ASN1_SEQUENCE_END(VM_PAYLOAD);

DECLARE_ASN1_FUNCTIONS(VM_PAYLOAD);
IMPLEMENT_ASN1_FUNCTIONS(VM_PAYLOAD);

typedef struct {
    ASN1_INTEGER *attestation_version;
    ASN1_OCTET_STRING *attestation_challenge;
    VM_ROOT_OF_TRUST *vm_root_of_trust;
    VM_PAYLOAD *vm_payload;
} AVF_VM_EXT;

ASN1_SEQUENCE(AVF_VM_EXT) = {
        ASN1_SIMPLE(AVF_VM_EXT, attestation_version, ASN1_INTEGER),
        ASN1_SIMPLE(AVF_VM_EXT, attestation_challenge, ASN1_OCTET_STRING),
        ASN1_SIMPLE(AVF_VM_EXT, vm_payload, VM_PAYLOAD),
        ASN1_SIMPLE(AVF_VM_EXT, vm_root_of_trust, VM_ROOT_OF_TRUST),
} ASN1_SEQUENCE_END(AVF_VM_EXT);

DECLARE_ASN1_FUNCTIONS(AVF_VM_EXT);
IMPLEMENT_ASN1_FUNCTIONS(AVF_VM_EXT);

X509_EXTENSION *generate_avf_extension(const struct avf_extension_details *details) {
    X509_EXTENSION *ex = NULL;
    AVF_VM_EXT *vm = NULL;
    ASN1_OCTET_STRING *vm_octet_string = NULL;
    unsigned char *vm_der = NULL;
    int vm_der_size;

    /* Fill in all the details. */
    vm = AVF_VM_EXT_new();
    if (!vm || !ASN1_INTEGER_set(vm->attestation_version, 1) ||
        !ASN1_OCTET_STRING_set(vm->attestation_challenge, details->challenge,
                               details->challenge_size)) {
        goto out;
    }

    if (!(vm->vm_root_of_trust = VM_ROOT_OF_TRUST_new()) ||
        !ASN1_OCTET_STRING_set(vm->vm_root_of_trust->verified_boot_key,
                               details->vm_root_of_trust.verified_boot_key,
                               details->vm_root_of_trust.verified_boot_key_size) ||
        !ASN1_ENUMERATED_set(vm->vm_root_of_trust->verified_boot_state,
                             details->vm_root_of_trust.verified_boot_state)) {
        goto out;
    }
    vm->vm_root_of_trust->device_unlocked = details->vm_root_of_trust.device_unlocked ? 1 : 0;
    vm->vm_root_of_trust->debuggable = details->vm_root_of_trust.debuggable ? 1 : 0;

    if (!(vm->vm_payload = VM_PAYLOAD_new()) ||
        !ASN1_OCTET_STRING_set(vm->vm_payload->authority, details->vm_payload.authority,
                               details->vm_payload.authority_size) ||
        !ASN1_OCTET_STRING_set(vm->vm_payload->digest, details->vm_payload.digest,
                               details->vm_payload.digest_size) ||
        !ASN1_OCTET_STRING_set(vm->vm_payload->binary_path, details->vm_payload.binary_path,
                               details->vm_payload.binary_path_size)) {
        goto out;
    }

    /* Convert to DER and embed in an octet string. */
    if ((vm_der_size = i2d_AVF_VM_EXT(vm, &vm_der)) < 0 ||
        !(vm_octet_string = ASN1_OCTET_STRING_new()) ||
        !ASN1_OCTET_STRING_set(vm_octet_string, vm_der, vm_der_size)) {
        goto out;
    }

    ex = X509_EXTENSION_create_by_NID(NULL, details->nid, /*crit=*/0, vm_octet_string);

out:
    ASN1_OCTET_STRING_free(vm_octet_string);
    OPENSSL_free(vm_der);
    AVF_VM_EXT_free(vm);
    return ex;
}
