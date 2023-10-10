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

#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <sys/cdefs.h>

#include "vm_payload.h"

#if !defined(__INTRODUCED_IN)
#define __INTRODUCED_IN(__api_level) /* nothing */
#endif

// The functions declared here are restricted to VMs created with a config file;
// they will fail if called in other VMs. The ability to create such VMs
// requires the android.permission.USE_CUSTOM_VIRTUAL_MACHINE permission, and is
// therefore not available to privileged or third party apps.

// These functions can be used by tests, if the permission is granted via shell.

__BEGIN_DECLS

struct AAttestationResult;

/**
 * Remote attestation status types returned from remote attestation functions.
 */
typedef enum {
    /** The remote attestatio completes successfully. */
    ATTESTATION_OK = 0,

    ATTESTATION_UNKNOWN_ERROR = -10000,

    /** The challenge size is not between 0 and 64. */
    ATTESTATION_ERROR_INVALID_CHALLENGE = ATTESTATION_UNKNOWN_ERROR - 1,
} attestation_status_t;

/**
 * Get the VM's DICE attestation chain.
 *
 * \param data pointer to size bytes where the chain is written (may be null if size is 0).
 * \param size number of bytes that can be written to data.
 *
 * \return the total size of the chain
 */
size_t AVmPayload_getDiceAttestationChain(void* _Nullable data, size_t size);

/**
 * Get the VM's DICE attestation CDI.
 *
 * \param data pointer to size bytes where the CDI is written (may be null if size is 0).
 * \param size number of bytes that can be written to data.
 *
 * \return the total size of the CDI
 */
size_t AVmPayload_getDiceAttestationCdi(void* _Nullable data, size_t size);

/**
 * Requests the remote attestation of the client VM.
 *
 * The challenge will be included in the certificate chain in the attestation result,
 * serving as proof of the freshness of the result.
 *
 * \param challenge A pointer to the challenge buffer.
 * \param challenge_size size of the challenge, the maximum supported challenge size is
 *          64 bytes. The status `attestation_status_t::ATTESTATION_ERROR_INVALID_CHALLENGE`
 *          will be returned if an invalid challenge is passed.
 * \param result The remote attestation result will be filled here if the attestation
 *               succeeds. The result remains valid until it is freed with
 *              `AVmPayload_freeAttestationResult`.
 *
 * \return ATTESTATION_OK on successful attestation.
 */
attestation_status_t AVmPayload_requestAttestation(
        const void* _Nonnull challenge, size_t challenge_size,
        struct AAttestationResult* _Nullable* _Nonnull result) __INTRODUCED_IN(__ANDROID_API_V__);

/**
 * Frees all the data owned by the provided attestation result, including the result itself.
 *
 * \param result A pointer to the attestation result.
 */
void AVmPayload_freeAttestationResult(struct AttestationResult* _Nonnull result)
        __INTRODUCED_IN(__ANDROID_API_V__);

/**
 * Reads the certificate chain from the provided attestation result. The certificate chain
 * consists of a sequence of DER-encoded X.509 certificates that form the attestation key's
 * certificate chain.
 *
 * \param data A pointer to the memory where the certificate chain will be written
 *             (can be null if size is 0).
 * \param size The maximum number of bytes that can be written to the data buffer. If `size`
 *             is smaller than the total size of the certificate chain, the chain will be
 *             truncated to this `size`.
 *
 * \return The total size of the certificate chain.
 */
size_t AVmPayload_getCertificateChainFromResult(struct AAttestationResult* _Nonnull result,
                                                void* _Nullable data, size_t size)
        __INTRODUCED_IN(__ANDROID_API_V__);

__END_DECLS
