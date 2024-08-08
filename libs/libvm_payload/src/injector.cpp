// Copyright 2024, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#include <android/log.h>
#include <include-restricted/vm_payload_restricted.h>

#define TAG "injector"

__attribute__((constructor)) void injectServices(void) {
    // All VM payloads have access to these host services that virtmgr allows
    // for this specific VM instance.
    AVmPayload_injectHostRpcServices();
    __android_log_write(ANDROID_LOG_INFO, TAG, "Injected host RPC services");
}
