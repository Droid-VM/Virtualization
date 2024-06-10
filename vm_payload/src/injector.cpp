#include <aidl/android/os/BnAccessor.h>
// #include <aidl/android/system/virtualization/payload/IVmPayloadService.h>
#include <android/binder_manager.h>
#include <android/log.h>
#include <vm_payload.h>

#define TAG "injectornator"

// FIXME get the remote IAccessor from microdroid_manager instead
class TODOAccessor : public aidl::android::os::BnAccessor {
    ::ndk::ScopedAStatus connectToRpcSession(::ndk::ScopedFileDescriptor*) override {
        return ::ndk::ScopedAStatus::ok();
    }
};

ndk::SpAIBinder gAccessor;

// This currently gets a new one every time. We should setOnUnlinked and remove
// them? No, I think libbinder will need to do the deleting, when it's done with
// the IAccesor somehow. FIXME come back to this, it's important.
AIBinder* generateIAccessor(const char* instance) {
    __android_log_write(ANDROID_LOG_INFO, TAG, "generating accessor for:");
    __android_log_write(ANDROID_LOG_INFO, TAG, instance);

    // Get the accessor from IVmPayloadService.
    // Do this in rust so we don't need to include the ndk library
    AIBinder* binder = AVmPayload_getIAccessor();
    if (binder) {
        __android_log_write(ANDROID_LOG_INFO, TAG, "generateIAccessor got accessor");
        gAccessor = ndk::SpAIBinder(binder);
        return gAccessor.get();
    } else {
        __android_log_write(ANDROID_LOG_INFO, TAG, "generateIAccessor failed to get accessor");
        return nullptr;
    }
}

__attribute__((constructor)) void injectServices(void) {
    __android_log_write(ANDROID_LOG_INFO, TAG, "injectServices");

    binder_exception_t status =
            AServiceManager_injectAccessorWithFlags(generateIAccessor,
                                                    "android.hardware.light.ILights/default",
                                                    AServiceManager_AddServiceFlag::
                                                            ADD_SERVICE_NONE);
    __android_log_write(ANDROID_LOG_INFO, TAG, "injectAccessor returned: ");
    __android_log_write(ANDROID_LOG_INFO, TAG, std::to_string(status).c_str());
}
