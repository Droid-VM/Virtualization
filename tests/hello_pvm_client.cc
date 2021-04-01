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

#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <thread>

#include "android-base/logging.h"
#include "android-base/parseint.h"
#include "android-base/unique_fd.h"

#include <BnHelloPVM.h>
#include <binder/Binder.h>
#include <binder/BpBinder.h>
#include <binder/IServiceManager.h>
#include <binder/ProcessState.h>
#include <binder/RpcConnection.h>
#include <binder/RpcServer.h>

#include <openssl/base64.h>
#include <openssl/pem.h>
#include <openssl/rsa.h>

using namespace android;
using namespace android::base;
using namespace android::binder;

static constexpr const char kDescriptor[] = "com.android.hello_pvm";
static constexpr const char kTestString[] = "Lorem Ipsum Dolor Sit Amet";
static constexpr unsigned int kPort = 12345;

static constexpr int kDigestNid = NID_sha256;
static constexpr const char kPublicKey[] =
        "-----BEGIN RSA PUBLIC KEY-----\n"
        "MIICCgKCAgEAsLHs87VpT5krCcDYcH1wZakljI0fSsz9LDt92BafjGOhtC/fjUdk\n"
        "eodt/GWq+8yNIpc4eyXlnJDOpQXvAMecWTKukTyda12dgWEES/DvifAA0oy4vwFY\n"
        "Ub64REau6M1MU+Y5eFmmWJOMlv7Lq2I0i0JGB2JPyRIZpl/YSft5aiu5Zcvng6C2\n"
        "h8NKFh/IagbfiaCNnHQS9LHCAhQGblMwArks9pLoRWCZUoQ1QnqVRRLFOZLDaaZA\n"
        "1U1a0DyylWtSC0st38UrPYeWKk1W7qRTVLhfX13kvoD2Z6u6gQDbYwtTKkG3ZlIX\n"
        "dfCUCBir0VSMmSadB+NNsFUaS/vMS7n1Y8bDa0/GYVAoXagKp30xpeZd3fa9j66I\n"
        "Tby0pcETBmnm3ZADZFY+IcCLc421Ex3W2IZWyiFbBU3q7rZdGJP81edYS+TVSgd2\n"
        "N9IR52+CQuuDqq4VDOT2jgYFVGlc6zWTCVnvGN9W6WHWr3Nrwh+M4OExq66Ednmf\n"
        "yTgy2Oo8om5YAk9dmLJLppnoOSvKWNt+vq8aXxPfGDHYu6gH1BgjwGiFtZMmBC5e\n"
        "l2xAM5XzZ1IGhrJ2RxHyBZaAeKEekYCGDtEm0NNIIxiYP66e807dqct40RBcaDux\n"
        "DrbU8O13voupWw9GVU2ly7nRREAlWMFSkizXgTA/4qTNYM676xRcA1MCAwEAAQ==\n"
        "-----END RSA PUBLIC KEY-----\n";

static std::vector<uint8_t> digest(const String16 &str) {
    std::vector<uint8_t> dig(EVP_MAX_MD_SIZE);
    unsigned int diglen = dig.size();
    CHECK(EVP_Digest(str.string(), str.size(), dig.data(), &diglen, EVP_get_digestbynid(kDigestNid),
                     NULL));
    dig.resize(diglen);
    return dig;
}

static bool verify(const String16 &str, const std::vector<uint8_t> &sig) {
    bssl::UniquePtr<BIO> bio(BIO_new_mem_buf(kPublicKey, sizeof(kPublicKey) - 1));
    CHECK(bio);
    bssl::UniquePtr<RSA> rsa(PEM_read_bio_RSAPublicKey(bio.get(), NULL, NULL, NULL));
    CHECK(rsa);
    const std::vector<uint8_t> dig = digest(str);
    return RSA_verify(kDigestNid, dig.data(), dig.size(), sig.data(), sig.size(), rsa.get());
}

static std::string base64(const std::vector<uint8_t> &data) {
    size_t len;
    CHECK(EVP_EncodedLength(&len, data.size()));
    std::vector<uint8_t> chars(len);
    CHECK_EQ(EVP_EncodeBlock(chars.data(), data.data(), data.size()), len - 1);
    return std::string(chars.begin(), chars.end());
}

int main(int argc, const char *argv[]) {
    SetLogger(StderrLogger);

    sp<RpcConnection> conn;
    sp<IBinder> obj;

    CHECK(argc >= 2);

    if (strcmp(argv[1], "ipc") == 0) {
        std::cout << "Quering ServiceManager for " << kDescriptor << "... " << std::flush;
        obj = defaultServiceManager()->getService(String16(kDescriptor));
        CHECK(obj);
        std::cout << "DONE" << std::endl;
    } else {
        CHECK(strcmp(argv[1], "rpc_vsock") == 0);

        unsigned int cid;
        CHECK(argc >= 3);
        CHECK(ParseUint(argv[2], &cid));

        std::cout << "Connecting to vsock:" << cid << ":" << kPort << "... " << std::flush;
        conn = RpcConnection::make();
        CHECK(conn->addVsockClient(cid, kPort));
        obj = conn->getRootObject();
        std::cout << "DONE" << std::endl;
    }

    sp<IHelloPVM> pvm = interface_cast<IHelloPVM>(obj);

    std::string where;
    std::cout << "$ whereAreYou()" << std::endl;
    CHECK(pvm->whereAreYou(&where).isOk());
    std::cout << "> \"" << where << "\"" << std::endl;

    std::cout << std::endl;

    String16 str(kTestString);
    std::vector<uint8_t> sig;
    std::cout << "$ toLower(\"" << str << "\")" << std::endl;
    CHECK(pvm->toLower(str, &sig, &str).isOk());
    std::cout << "> \"" << str << "\"" << std::endl;
    std::cout << "> sig=" << base64(sig).substr(0, 64) << "..." << std::endl;
    std::cout << "> verify=" << verify(str, sig) << std::endl;

    return EXIT_SUCCESS;
}
