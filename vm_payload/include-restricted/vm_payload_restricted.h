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

struct AttestationResult;

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
 *                       64 bytes.
 *
 * \return the attestation result.
 */
struct AttestationResult* _Nonnull AVmPayload_requestAttestation(const void* _Nonnull challenge,
                                                                 size_t challenge_size)
        __INTRODUCED_IN(__ANDROID_API_V__);

/**
 * Frees all the data owned by the provided attestation result, including the result itself.
 *
 * \param result A pointer to the attestation result.
 */
void AVmPayload_freeAttestationResult(struct AttestationResult* _Nonnull result)
        __INTRODUCED_IN(__ANDROID_API_V__);

/**
 * Reads the certificate chain from the given attestation result.
 *
 * \param data A pointer to the memory where the certificate chain will be written
 *             (can be null if size is 0).
 * \param size The maximum number of bytes that can be written to the data buffer.
 *
 * \return The total size of the certificate chain.
 */
size_t AVmPayload_getCertificateChainFromResult(struct AttestationResult* _Nonnull result,
                                                void* _Nullable data, size_t size)
        __INTRODUCED_IN(__ANDROID_API_V__);

__END_DECLS
