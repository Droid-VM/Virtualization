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

#include "io_authfs.h"

#include <android-base/logging.h>
#include <binder/IServiceManager.h>
#include <com/android/virt/fs/AuthFsConfig.h>
#include <com/android/virt/fs/IAuthFs.h>
#include <com/android/virt/fs/IAuthFsService.h>
#include <time.h>

#include <random>

using namespace android;
using namespace com::android::virt::fs;

using android::base::ErrnoError;
using android::base::Error;
using android::base::Result;
using android::os::ParcelFileDescriptor;

namespace io_authfs {
constexpr uint64_t kBlockSizeBytes = 4096;
constexpr uint64_t kNumBytesPerMB = 1024 * 1024;

Result<double> measure_read_rate(int remote_fd, long file_size_bytes, bool is_rand) {
    LOG(INFO) << "VM:Start measuring read rate.";
    sp<IAuthFsService> authfs_service = waitForService<IAuthFsService>(String16("authfs_service"));
    if (authfs_service == nullptr) {
        return Error() << "AuthFsService is null";
    }
    AuthFsConfig authfs_config;
    authfs_config.port = 3264;
    sp<IAuthFs> auth_fs;
    auto status = authfs_service->mount(authfs_config, &auth_fs);
    if (!status.isOk()) {
        return Error() << "Failed AuthFsService#mount(), status:" << status;
    }
    ParcelFileDescriptor fd;
    status = auth_fs->openFile(remote_fd, /*writable=*/false, &fd);
    if (!status.isOk()) {
        return Error() << "Failed AuthFs#openFile(), status:" << status;
    }
    LOG(INFO) << "VM:Fetched file.";
    const int64_t block_count = file_size_bytes / kBlockSizeBytes;
    std::vector<uint64_t> offsets;
    if (is_rand) {
        std::mt19937 rd{std::random_device{}()};
        offsets.reserve(block_count);
        for (auto i = 0; i < block_count; ++i) offsets.push_back(i * kBlockSizeBytes);
        std::shuffle(offsets.begin(), offsets.end(), rd);
    }
    char buf[kBlockSizeBytes];

    clock_t start = clock();
    for (auto i = 0; i < block_count; ++i) {
        if (is_rand) {
            if (lseek(fd.get(), offsets[i], SEEK_SET) == -1) {
                return ErrnoError() << "failed to lseek";
            }
        }
        auto bytes = read(fd.get(), buf, kBlockSizeBytes);
        if (bytes == 0) {
            return Error() << "unexpected end of file";
        } else if (bytes == -1) {
            return ErrnoError() << "failed to read";
        }
    }
    double elapsed_seconds = ((double)clock() - start) / CLOCKS_PER_SEC;
    double read_rate = (double)file_size_bytes / kNumBytesPerMB / elapsed_seconds;
    LOG(INFO) << "VM:Finished reading data with rate " << read_rate << "Mb/s";
    return {read_rate};
}
} // namespace io_authfs
