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
#include <iostream>
#include <thread>
#include <vector>

#include <android-base/properties.h>
#include "android-base/logging.h"
#include "android-base/parseint.h"
#include "android-base/unique_fd.h"

#include <BnHelloPVM.h>
#include <binder/Binder.h>
#include <binder/BpBinder.h>
#include <binder/IPCThreadState.h>
#include <binder/IServiceManager.h>
#include <binder/ProcessState.h>
#include <binder/RpcConnection.h>
#include <binder/RpcServer.h>

#include <openssl/aes.h>
#include <openssl/err.h>
#include <openssl/evp.h>
#include <openssl/pem.h>
#include <openssl/rsa.h>

using namespace android;
using namespace android::base;
using namespace android::binder;

static constexpr const char kDescriptor[] = "com.android.hello_pvm";
static constexpr unsigned int kPort = 12345;
static constexpr size_t kNumThreads = 4;

static constexpr int kDigestNid = NID_sha256;
static constexpr const char kPrivateKey[] =
        "-----BEGIN RSA PRIVATE KEY-----\n"
        "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\n"
        "-----END RSA PRIVATE KEY-----\n";

class MyHelloPVM : public BnHelloPVM {
public:
    Status whereAreYou(std::string *str) override {
        std::cout << __PRETTY_FUNCTION__ << std::endl;

        *str = GetProperty("ro.hardware", "n/a");
        return Status::ok();
    }

    Status toLower(const String16 &in, std::vector<uint8_t> *outsig, String16 *out) {
        String16 tmp = in;
        CHECK(tmp.makeLower() == 0);

        *out = tmp;
        *outsig = sign(tmp);
        return Status::ok();
    }

private:
    static std::vector<uint8_t> digest(const String16 &str) {
        std::vector<uint8_t> dig(EVP_MAX_MD_SIZE);
        unsigned int diglen = dig.size();
        CHECK(EVP_Digest(str.string(), str.size(), dig.data(), &diglen,
                         EVP_get_digestbynid(kDigestNid), NULL));
        dig.resize(diglen);
        return dig;
    }

    static std::vector<uint8_t> sign(const String16 &str) {
        bssl::UniquePtr<BIO> bio(BIO_new_mem_buf(kPrivateKey, sizeof(kPrivateKey) - 1));
        CHECK(bio);
        bssl::UniquePtr<RSA> rsa(PEM_read_bio_RSAPrivateKey(bio.get(), NULL, NULL, NULL));
        CHECK(rsa);

        const std::vector<uint8_t> dig = digest(str);
        std::vector<uint8_t> sig(RSA_size(rsa.get()));
        unsigned int siglen = sig.size();
        CHECK(RSA_sign(kDigestNid, dig.data(), dig.size(), sig.data(), &siglen, rsa.get()));
        sig.resize(siglen);
        return sig;
    }
};

int main(int argc, const char *argv[]) {
    SetLogger(StderrLogger);

    CHECK(argc == 2);
    if (strcmp(argv[1], "ipc") == 0) {
        std::cout << "Registering with ServiceManager... " << std::flush;
        defaultServiceManager()->addService(String16(kDescriptor), new MyHelloPVM);
        std::cout << "DONE" << std::endl;

        std::cout << "Starting thread pool... " << std::endl;

        // start the thread pool
        ProcessState::self()->setThreadPoolMaxThreadCount(4);
        sp<ProcessState> ps(ProcessState::self());
        ps->startThreadPool();
        ps->giveThreadPoolName();
        IPCThreadState::self()->joinThreadPool();
    } else {
        CHECK(strcmp(argv[1], "rpc_vsock") == 0);

        std::cout << "Creating RPC server... " << std::flush;
        sp<RpcServer> server = RpcServer::make();
        server->iUnderstandThisCodeIsExperimentalAndIWillNotUseItInProduction();
        server->setRootObject(new MyHelloPVM);
        std::cout << "DONE" << std::endl;

        std::cout << "Listening on vsock port " << kPort << "... " << std::endl;
        sp<RpcConnection> conn = server->addClientConnection();
        CHECK(conn->setupVsockServer(kPort));

        std::vector<std::thread> pool;
        for (size_t i = 0; i + 1 < kNumThreads; i++) {
            pool.push_back(std::thread([=] { conn->join(); }));
        }
        conn->join();
        for (auto &t : pool) t.join();
    }
}
