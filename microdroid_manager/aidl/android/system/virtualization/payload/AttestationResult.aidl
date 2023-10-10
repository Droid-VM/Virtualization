/*
 * Copyright 2023 The Android Open Source Project
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

package android.system.virtualization.payload;

/**
 * An {@link AttestationResult} holds an attested private key and the remotely
 * provisioned certificate chain covering its corresponding public key.
 *
 * @hide
 */
parcelable AttestationResult {
    /**
     * COSE_Key encoded EC P-256 private key, which is attested.
     *
     * The corresponding public key is included in the leaf certificate of
     * the certificate chain.
     */
    byte[] privateKey;

    /**
     * Sequence of DER-encoded X.509 certificates that make up the attestation
     * key's certificate chain.
     */
    byte[] certificateChain;
}
