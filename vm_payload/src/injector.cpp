#include <aidl/android/os/BnAccessor.h>
#include <android/binder_manager.h>
#include <android/log.h>

#define TAG "injectornator"

// FIXME get the remote IAccessor from microdroid_manager instead
class TODOAccessor : public aidl::android::os::BnAccessor {
    ::ndk::ScopedAStatus connectToRpcSession(::ndk::ScopedFileDescriptor*) override {
        return ::ndk::ScopedAStatus::ok();
    }
};

std::shared_ptr<TODOAccessor> gAccessor;

AIBinder* generateIAccessor(const char* instance) {
    __android_log_write(ANDROID_LOG_INFO, TAG, "generating accessor for:");
    __android_log_write(ANDROID_LOG_INFO, TAG, instance);

    return gAccessor->asBinder().get();
}

__attribute__((constructor)) void injectServices(void) {
    __android_log_write(ANDROID_LOG_INFO, TAG, "injectServices");

    binder_exception_t status =
            AServiceManager_injectAccessorWithFlags(generateIAccessor,
                                                    "android.hardware.lights.ILights/default",
                                                    AServiceManager_AddServiceFlag::
                                                            ADD_SERVICE_NONE);
    __android_log_write(ANDROID_LOG_INFO, TAG, "injectAccessor returned: ");
    __android_log_write(ANDROID_LOG_INFO, TAG, std::to_string(status).c_str());
}
