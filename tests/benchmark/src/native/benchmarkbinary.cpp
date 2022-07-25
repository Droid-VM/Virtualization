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
#include <aidl/android/system/virtualmachineservice/IVirtualMachineService.h>
#include <aidl/com/android/microdroid/testservice/BnReportService.h>
#include <android-base/result.h>
#include <android-base/unique_fd.h>
#include <fcntl.h>
#include <linux/vm_sockets.h>
#include <stdio.h>
#include <unistd.h>

#include <binder_rpc_unstable.hpp>
#include <chrono>
#include <random>
#include <string>
#include <vector>

#include "android-base/logging.h"

constexpr uint64_t BLOCK_SIZE = 4096; // bytes
constexpr uint64_t BYTES_PER_MB = 1024 * 1024;

using aidl::android::system::virtualmachineservice::IVirtualMachineService;
using android::base::ErrnoError;
using android::base::Error;
using android::base::Result;
using android::base::unique_fd;

namespace {
Result<void> run_virtio_blk_benchmark_tests() {
    class VirtioBlkReportService
          : public aidl::com::android::microdroid::testservice::BnReportService {
    public:
        ndk::ScopedAStatus getSeqReadRate(double* out) override {
            *out = _seq_read_rate;
            return ndk::ScopedAStatus::ok();
        }

        ndk::ScopedAStatus getRandReadRate(double* out) override {
            *out = _rand_read_rate;
            return ndk::ScopedAStatus::ok();
        }

        Result<void> run_tests() {
            // TODO: Read microdroid_super.img here
            const char* filename = "/system/apex/com.android.runtime.apex";
            unique_fd fd(open(filename, O_RDONLY));
            if (fd.get() == -1) {
                return ErrnoError() << "opening " << filename << " failed";
            }
            char buf[BLOCK_SIZE] = {};
            uint64_t block_count = 0;
            // Gets the number of blocks.
            while (read(fd.get(), buf, BLOCK_SIZE) > 0) {
                ++block_count;
            }
            if (lseek(fd.get(), 0, SEEK_SET) == -1) {
                return ErrnoError() << "failed to lseek";
            }

            // Test sequential read.
            clock_t start = clock();
            for (uint64_t i = 0; i < block_count; i++) {
                auto bytes = read(fd.get(), buf, BLOCK_SIZE);
                if (bytes == 0) {
                    return Error() << "unexpected end of file";
                } else if (bytes == -1) {
                    return ErrnoError() << "failed to read";
                }
            }
            double elapsed_seconds = ((double)clock() - start) / CLOCKS_PER_SEC;
            const double mb = block_count * BLOCK_SIZE / BYTES_PER_MB;
            _seq_read_rate = mb / elapsed_seconds;

            std::vector<uint64_t> offsets;
            std::mt19937 rd{std::random_device{}()};
            offsets.reserve(block_count);
            for (uint64_t i = 0; i < block_count; i++) offsets.push_back(i * BLOCK_SIZE);
            std::shuffle(offsets.begin(), offsets.end(), rd);

            // Test random read.
            start = clock();
            for (uint64_t i = 0; i < block_count; i++) {
                if (lseek(fd.get(), offsets[i], SEEK_SET) == -1) {
                    return ErrnoError() << "failed to lseek";
                }
                auto bytes = read(fd.get(), buf, BLOCK_SIZE);
                if (bytes == 0) {
                    return Error() << "unexpected end of file";
                } else if (bytes == -1) {
                    return ErrnoError() << "failed to read";
                }
            }
            elapsed_seconds = ((double)clock() - start) / CLOCKS_PER_SEC;
            _rand_read_rate = mb / elapsed_seconds;
            return {};
        }

    private:
        double _seq_read_rate;
        double _rand_read_rate;
    };
    auto test_service = ndk::SharedRefBase::make<VirtioBlkReportService>();
    if (auto res = test_service->run_tests(); !res.ok()) {
        return res.error();
    }
    auto callback = []([[maybe_unused]] void* param) {
        // Tell microdroid_manager that we're ready.
        // If we can't, abort in order to fail fast - the host won't proceed without
        // receiving the onReady signal.
        ndk::SpAIBinder binder(
                RpcClient(VMADDR_CID_HOST, IVirtualMachineService::VM_BINDER_SERVICE_PORT));
        auto virtualMachineService = IVirtualMachineService::fromBinder(binder);
        if (virtualMachineService == nullptr) {
            LOG(ERROR) << "failed to connect VirtualMachineService\n";
            abort();
        }
        if (auto status = virtualMachineService->notifyPayloadReady(); !status.isOk()) {
            LOG(ERROR) << "failed to notify payload ready to virtualizationservice: "
                       << status.getDescription();
            abort();
        }
    };

    if (!RunRpcServerCallback(test_service->asBinder().get(), test_service->SERVICE_PORT, callback,
                              nullptr)) {
        return Error() << "RPC Server failed to run";
    }
    return {};
}
} // Anonymous namespace

extern "C" int android_native_main([[maybe_unused]] int argc, [[maybe_unused]] char* argv[]) {
    if (strcmp(argv[1], "no_io") == 0) {
        // do nothing for now; just leave it alive. good night.
        for (;;) {
            sleep(1000);
        }
    } else if (strcmp(argv[1], "io") == 0) {
        if (auto res = run_virtio_blk_benchmark_tests(); res.ok()) {
            return 0;
        } else {
            LOG(ERROR) << "IO benchmark test failed: " << res.error() << "\n";
            return 1;
        }
    }
    return 0;
}
