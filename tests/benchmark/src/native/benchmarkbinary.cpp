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
#include <numeric>
#include <random>
#include <string>
#include <unordered_map>
#include <vector>

#include "android-base/logging.h"

constexpr uint64_t kBlockSize = 4096; // bytes
constexpr uint64_t kBytesPerMb = 1024 * 1024;

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
        ndk::ScopedAStatus getDouble(const std::string& key, double* out) override {
            const auto entry = _metrics.find(key);
            if (entry == _metrics.end()) {
                return ndk::ScopedAStatus::fromExceptionCodeWithMessage(EX_ILLEGAL_ARGUMENT,
                                                                        "Cannot find key");
            }
            *out = entry->second;
            return ndk::ScopedAStatus::ok();
        }

        Result<void> run_tests() {
            const char* filename = "/apex/com.android.virt/etc/fs/microdroid_super.img";
            const int n = 5; // Times to run the I/O tests.
            unique_fd fd(open(filename, O_RDONLY));
            if (fd.get() == -1) {
                return ErrnoError() << "opening " << filename << " failed";
            }
            char buf[kBlockSize] = {};
            uint64_t block_count = 0;
            while (read(fd.get(), buf, kBlockSize) > 0) {
                ++block_count;
            }

            if (auto res = run_read_file_test(fd, block_count, /*is_rand=*/false); !res.ok()) {
                return res.error();
            }
            if (auto res = run_read_file_test(fd, block_count, /*is_rand=*/true); !res.ok()) {
                return res.error();
            }
            return {};
        }

    private:
        Result<void> run_read_file_test(unique_fd& fd, uint64_t block_count, bool is_rand) {
            const int trial_count = 10;
            const double mb = block_count * kBlockSize / kBytesPerMb;
            std::vector<double> elapsed_seconds;
            for (int i = 0; i < trial_count; ++i) {
                if (auto res = read_file(fd, block_count, is_rand); res.ok()) {
                    elapsed_seconds.push_back(mb / res.value());
                } else {
                    return res.error();
                }
            }

            double mean = std::accumulate(elapsed_seconds.begin(), elapsed_seconds.end(), 0.0) /
                    elapsed_seconds.size();
            double sq_sum = std::inner_product(elapsed_seconds.begin(), elapsed_seconds.end(),
                                               elapsed_seconds.begin(), 0.0);
            double stdev = std::sqrt(sq_sum / elapsed_seconds.size() - mean * mean);
            if (is_rand) {
                _metrics[RAND_READ_MEAN] = mean;
                _metrics[RAND_READ_STD] = stdev;
            } else {
                _metrics[SEQ_READ_MEAN] = mean;
                _metrics[SEQ_READ_STD] = stdev;
            }
            return {};
        }

        /** Returns the elapsed seconds for reading the given file. */
        Result<double> read_file(unique_fd& fd, uint64_t block_count, bool is_rand) {
            std::vector<uint64_t> offsets;
            if (is_rand) {
                std::mt19937 rd{std::random_device{}()};
                offsets.reserve(block_count);
                for (uint64_t i = 0; i < block_count; i++) offsets.push_back(i * kBlockSize);
                std::shuffle(offsets.begin(), offsets.end(), rd);
            }
            char buf[kBlockSize] = {};
            if (lseek(fd.get(), 0, SEEK_SET) == -1) {
                return ErrnoError() << "failed to lseek";
            }
            sync();

            clock_t start = clock();
            for (uint64_t i = 0; i < block_count; i++) {
                if (is_rand) {
                    if (lseek(fd.get(), offsets[i], SEEK_SET) == -1) {
                        return ErrnoError() << "failed to lseek";
                    }
                }
                auto bytes = read(fd.get(), buf, kBlockSize);
                if (bytes == 0) {
                    return Error() << "unexpected end of file";
                } else if (bytes == -1) {
                    return ErrnoError() << "failed to read";
                }
            }
            return {((double)clock() - start) / CLOCKS_PER_SEC};
        }

        std::unordered_map<std::string, double> _metrics;
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

extern "C" int android_native_main([[maybe_unused]] int argc, char* argv[]) {
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
