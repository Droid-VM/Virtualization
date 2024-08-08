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

#include <android/binder_manager.h>
#include <android/binder_rpc.h>
#include <android/log.h>
#include <vm_payload.h>

#include <string>

#define TAG "injector"

ABinderRpc_Accessor* accessorProvider(const char* instance, void*) {
    // Get the accessor from IVmPayloadService.
    // Do this in rust so we don't need to include the ndk library
    AIBinder* accessor = AVmPayload_getAccessorBinder(instance);
    if (accessor == nullptr) {
        __android_log_write(ANDROID_LOG_INFO, TAG,
                            "AVmPayload_getAccessorBinder failed to get accessor");
        return nullptr;
    }

    return ABinderRpc_Accessor_fromBinder(instance, accessor);
}

__attribute__((constructor)) void injectServices(void) {
    // TODO get this from microdroidmgr through a new AVmPayload API.
    const char* kSupportedServices[] = {
            "android.frameworks.stats.IStats/default",
    };
    ABinderRpc_AccessorProvider* provider =
            ABinderRpc_registerAccessorProvider(accessorProvider, kSupportedServices, 1, nullptr,
                                                nullptr);

    if (provider) {
        __android_log_write(ANDROID_LOG_INFO, TAG, "Added host binder RPC services to VM payload!");
    } else {
        __android_log_write(ANDROID_LOG_INFO, TAG,
                            "Failed to add host binder RPC services to VM payload!");
    }
    // drop the pointer to the ABinderRpc_AccessorProvider since we never
    // intend to unregister the provider!
}
