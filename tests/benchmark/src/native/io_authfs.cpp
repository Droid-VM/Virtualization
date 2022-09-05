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

#include <binder/IServiceManager.h>
#include <com/android/virt/fs/AuthFsConfig.h>
#include <com/android/virt/fs/IAuthFs.h>
#include <com/android/virt/fs/IAuthFsService.h>

using namespace android;
using namespace com::android::virt::fs;

using android::base::Error;
using android::base::Result;

namespace io_authfs {
Result<void> run() {
    // sp<IAuthFsService> authfs_service =
    // waitForService<IAuthFsService>(String16("authfs_service"));
    sp<IAuthFsService> authfs_service = interface_cast<IAuthFsService>(
            defaultServiceManager()->getService(String16("authfs_service")));
    if (authfs_service == nullptr) {
        return Error() << "AuthFsService is null";
    }
    AuthFsConfig authfs_config;
    authfs_config.port = 3264;
    sp<IAuthFs> auth_fs;
    auto status = authfs_service->mount(authfs_config, &auth_fs);
    if (!status.isOk()) {
        return Error() << "Failed AuthFsService#mount()";
    }
    return {};
}

Result<double> measure_read_rate([[maybe_unused]] int remote_fd,
                                 [[maybe_unused]] long file_size_bytes,
                                 [[maybe_unused]] bool is_rand) {
    if (auto res = run(); !res.ok()) {
        return res.error();
    }
    return {1.0};
}
} // namespace io_authfs
