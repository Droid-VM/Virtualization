/*
 * Copyright 2022 The Android Open Source Project
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

#include <android-base/file.h>
#include <android-base/logging.h>
#include <openssl/evp.h>
#include <openssl/mem.h>
#include <unistd.h>
#include <vm_payload.h>

#include <string_view>
#include <vector>

#include "compos_key.h"

using android::base::Error;
using android::base::ReadFdToString;
using android::base::Result;
using android::base::WriteFully;
using namespace std::literals;
using compos_key::Ed25519KeyPair;
using compos_key::Seed;

namespace {

constexpr const char* kSigningKeySeedIdentifier = "CompOS signing key seed";

Result<Ed25519KeyPair> getSigningKey() {
    Seed seed;
    if (!AVmPayload_getVmInstanceSecret(kSigningKeySeedIdentifier,
                                        strlen(kSigningKeySeedIdentifier), seed.data(),
                                        seed.size())) {
        return Error() << "Failed to get signing key seed";
    }
    return compos_key::keyFromSeed(seed);
}

int write_public_key() {
    auto key_pair = getSigningKey();
    if (!key_pair.ok()) {
        LOG(ERROR) << key_pair.error();
        return 1;
    }
    if (!WriteFully(STDOUT_FILENO, key_pair->public_key.data(), key_pair->public_key.size())) {
        PLOG(ERROR) << "Write failed";
        return 1;
    }
    return 0;
}

int write_bcc() {
    uint8_t cert[2048];
    const char challenge[] = "Test challenge";

    // TODO: errors
    EVP_PKEY *key = NULL;
    EVP_PKEY_CTX *pctx = EVP_PKEY_CTX_new_id(EVP_PKEY_EC, NULL);
    EVP_PKEY_keygen_init(pctx);
    EVP_PKEY_CTX_set_ec_paramgen_curve_nid(pctx, NID_X9_62_prime256v1);
    EVP_PKEY_keygen(pctx, &key);
    EVP_PKEY_CTX_free(pctx);
    uint8_t *key_der = NULL;
    int len = i2d_PUBKEY(key, &key_der);
    if (len < 0) return 1;
    EVP_PKEY_free(key);

    size_t size = AVmPayload_getRemotelyAttestedCertificate(key_der, len, challenge,
                                                            strlen(challenge), cert, sizeof(cert));
    OPENSSL_free(key_der);
    if (size == 0) {
        LOG(ERROR) << "Failed to remotely attested cert";
        return 1;
    }

    if (!WriteFully(STDOUT_FILENO, cert, size)) {
        PLOG(ERROR) << "Write failed";
        return 1;
    }

    return 0;
}

int sign_input() {
    std::string to_sign;
    if (!ReadFdToString(STDIN_FILENO, &to_sign)) {
        PLOG(ERROR) << "Read failed";
        return 1;
    }

    auto key_pair = getSigningKey();
    if (!key_pair.ok()) {
        LOG(ERROR) << key_pair.error();
        return 1;
    }

    auto signature =
            compos_key::sign(key_pair->private_key,
                             reinterpret_cast<const uint8_t*>(to_sign.data()), to_sign.size());
    if (!signature.ok()) {
        LOG(ERROR) << signature.error();
        return 1;
    }

    if (!WriteFully(STDOUT_FILENO, signature->data(), signature->size())) {
        PLOG(ERROR) << "Write failed";
        return 1;
    }
    return 0;
}
} // namespace

int main(int argc, char** argv) {
    android::base::InitLogging(argv, android::base::LogdLogger(android::base::SYSTEM));

    if (argc == 2) {
        if (argv[1] == "public_key"sv) {
            return write_public_key();
        } else if (argv[1] == "bcc"sv) {
            return write_bcc();
        } else if (argv[1] == "sign"sv) {
            return sign_input();
        }
    }

    LOG(INFO) << "Usage: compos_key_helper <command>. Available commands are:\n"
                 "public_key   Write current public key to stdout\n"
                 "sign         Consume stdin, sign it and write signature to stdout\n";
    return 1;
}
