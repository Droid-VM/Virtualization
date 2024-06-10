// FIXME license
#include <android/binder_manager.h>
#include <android/binder_rpc.h>
#include <android/log.h>
#include <vm_payload.h>

#include <string>

#define TAG "injectornator"

ARpc_Accessor* accessorProvider(const char* instance, void* data) {
    (void)data;
    __android_log_write(ANDROID_LOG_INFO, TAG, "generating accessor for:");
    __android_log_write(ANDROID_LOG_INFO, TAG, instance);

    // Get the accessor from IVmPayloadService.
    // Do this in rust so we don't need to include the ndk library
    AIBinder* accessor = AVmPayload_getAccessorBinder(instance);
    if (accessor) {
        __android_log_write(ANDROID_LOG_INFO, TAG, "AVmPayload_getAccessorBinder got accessor");
    } else {
        __android_log_write(ANDROID_LOG_INFO, TAG,
                            "AVmPayload_getAccessorBinder failed to get accessor");
        return nullptr;
    }

    return ARpc_Accessor_fromBinder(instance, accessor);
}

__attribute__((constructor)) void injectServices(void) {
    __android_log_write(ANDROID_LOG_INFO, TAG, "injectServices");

    binder_exception_t status = ARpc_addAccessorProvider(accessorProvider, nullptr);

    __android_log_write(ANDROID_LOG_INFO, TAG, "addAccessorProvider returned: ");
    __android_log_write(ANDROID_LOG_INFO, TAG, std::to_string(status).c_str());
}
