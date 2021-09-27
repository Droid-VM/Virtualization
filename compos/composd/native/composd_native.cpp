/*
 * Copyright (C) 2021 The Android Open Source Project
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

#include "composd_native.h"

#include <algorithm>
#include <iterator>

using rust::Slice;
using rust::Vec;

Vec<uint8_t> extract_rsa_public_key(rust::Slice<const uint8_t> der_certificate) {
    Vec<uint8_t> result;

    auto data = der_certificate.data();
    bssl::UniquePtr<X509> x509(d2i_X509(nullptr, &data, der_certificate.size()));
    if (!x509 || data != der_certificate.data() + der_certificate.size()) {
        return result;
    }

    bssl::UniquePtr<EVP_PKEY> pkey(X509_get_pubkey(x509.get()));
    if (EVP_PKEY_base_id(pkey.get()) != EVP_PKEY_RSA) {
        return result;
    }
    RSA* rsa = EVP_PKEY_get0_RSA(pkey.get());
    if (!rsa) {
        return result;
    }

    uint8_t* out = nullptr;
    int size = i2d_RSAPublicKey(rsa, &out);
    if (size < 0 || !out) {
        return result;
    }
    bssl::UniquePtr<uint8_t> buffer(out);

    result.reserve(size);
    std::copy(out, out + size, std::back_inserter(result));
    return result;
}
