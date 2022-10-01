#ifndef VIRTUALIZATIONSERVICE_RKPVM_EXT_H
#define VIRTUALIZATIONSERVICE_RKPVM_EXT_H

#include <openssl/base.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum verified_boot_state {
    VERIFIED = 0,
    SELF_SIGNED = 1,
    UNVERIFIED = 3,
};

struct vm_root_of_trust_details {
    const uint8_t *verified_boot_key;
    size_t verified_boot_key_size;
    enum verified_boot_state verified_boot_state;
    bool device_unlocked;
    bool debuggable;
};

struct vm_payload_details {
    const uint8_t *authority;
    size_t authority_size;
    const uint8_t *digest;
    size_t digest_size;
    const uint8_t *binary_path;
    size_t binary_path_size;
};

struct avf_extension_details {
    int nid;
    const uint8_t *challenge;
    size_t challenge_size;
    struct vm_root_of_trust_details vm_root_of_trust;
    struct vm_payload_details vm_payload;
};

/**
 * Generates an AVF attestation certificate extension. Returns a pointer to the newly allocated
 * structure on success, or NULL on failure.
 */
X509_EXTENSION *generate_avf_extension(const struct avf_extension_details *details);

#endif // VIRTUALIZATIONSERVICE_RKPVM_EXT_H
