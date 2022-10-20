/*
 * Copyright 2021 The Android Open Source Project
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

#define LOG_TAG "VirtualMachine"

#include <aidl/android/system/virtualizationservice/IVirtualMachine.h>
#include <aidl/android/system/virtualizationservice/IVirtualizationService.h>
#include <android-base/unique_fd.h>
#include <android/binder_auto_utils.h>
#include <android/binder_ibinder_jni.h>
#include <jni.h>
#include <log/log.h>

#include <binder_rpc_unstable.hpp>
#include <tuple>

using namespace android::base;

static jmethodID sVirtualizationServiceCtor;

static constexpr const char VIRTMGR_PATH[] = "/apex/com.android.virt/bin/virtmgr";
static constexpr size_t VIRTMGR_THREADS = 16;

// Wrapper around ARpcSession handle that automatically frees the handle when
// it goes out of scope.
class RpcSessionHandle {
public:
    RpcSessionHandle() : mHandle(ARpcSession_new()) {}
    ~RpcSessionHandle() { ARpcSession_free(mHandle); }

    ARpcSession* get() { return mHandle; }

private:
    ARpcSession* mHandle;
};

JNIEXPORT jobject JNICALL android_system_virtualmachine_VirtualMachine_connectToVsockServer(
        JNIEnv* env, [[maybe_unused]] jclass clazz, jobject vmBinder, jint port) {
    using aidl::android::system::virtualizationservice::IVirtualMachine;
    using ndk::ScopedFileDescriptor;
    using ndk::SpAIBinder;

    auto vm = IVirtualMachine::fromBinder(SpAIBinder{AIBinder_fromJavaBinder(env, vmBinder)});

    std::tuple args{env, vm.get(), port};
    using Args = decltype(args);

    auto requestFunc = [](void* param) {
        auto [env, vm, port] = *static_cast<Args*>(param);

        ScopedFileDescriptor fd;
        if (auto status = vm->connectVsock(port, &fd); !status.isOk()) {
            env->ThrowNew(env->FindClass("android/system/virtualmachine/VirtualMachineException"),
                          ("Failed to connect vsock: " + status.getDescription()).c_str());
            return -1;
        }

        // take ownership
        int ret = fd.get();
        *fd.getR() = -1;

        return ret;
    };

    RpcSessionHandle session;
    auto client = ARpcSession_setupPreconnectedClient(session.get(), requestFunc, &args);
    return AIBinder_toJavaBinder(env, client);
}

JNIEXPORT jobject JNICALL android_system_virtualmachine_VirtualizationService_spawn(JNIEnv* env,
                                                                                    jclass clazz) {
    using aidl::android::system::virtualizationservice::IVirtualizationService;
    using ndk::ScopedFileDescriptor;
    using ndk::SpAIBinder;

    unique_fd serverFd, clientFd, waitFd, readyFd, keepAliveFd, shutdownFd;
    if (!Socketpair(SOCK_STREAM, &serverFd, &clientFd) || !Pipe(&waitFd, &readyFd, 0)) {
        env->ThrowNew(env->FindClass("android/system/virtualmachine/VirtualMachineException"),
                      "Failed to create socketpair/pipe");
        return nullptr;
    }

    if (fork() == 0) {
        // Close client's FDs.
        clientFd.reset();
        waitFd.reset();

        auto strServerFd = std::to_string(serverFd.get());
        auto strReadyFd = std::to_string(readyFd.get());

        execl(VIRTMGR_PATH, VIRTMGR_PATH, "--rpc-server-fd", strServerFd.c_str(), "--ready-fd",
              strReadyFd.c_str(), NULL);
    }

    // Close virtmgr's FDs.
    serverFd.reset();
    readyFd.reset();

    // Wait for the server to signal its readiness by closing its end of the pipe.
    char buf;
    if (read(waitFd.get(), &buf, sizeof(buf)) < 0) {
        env->ThrowNew(env->FindClass("android/system/virtualmachine/VirtualMachineException"),
                      "Failed to wait for VirtualizationService to be ready");
        return nullptr;
    }

    return env->NewObject(clazz, sVirtualizationServiceCtor, clientFd.release());
}

JNIEXPORT jobject JNICALL android_system_virtualmachine_VirtualizationService_connect(
        JNIEnv* env, [[maybe_unused]] jobject obj, int clientFd) {
    RpcSessionHandle session;
    ARpcSession_setFileDescriptorTransportMode(session.get(),
                                               ARpcSession_FileDescriptorTransportMode::Unix);
    ARpcSession_setMaxIncomingThreads(session.get(), VIRTMGR_THREADS);
    ARpcSession_setMaxOutgoingThreads(session.get(), VIRTMGR_THREADS);
    // SAFETY - ARpcSession_setupUnixDomainBootstrapClient does not take ownership of clientFd.
    auto client = ARpcSession_setupUnixDomainBootstrapClient(session.get(), clientFd);
    return AIBinder_toJavaBinder(env, client);
}

JNIEXPORT void JNICALL android_system_virtualmachine_VirtualizationService_finalize(
        [[maybe_unused]] JNIEnv* env, [[maybe_unused]] jobject obj, int clientFd) {
    // Close clientFd. The server will shut down in response to the HUP.
    unique_fd ufd(clientFd);
}

JNIEXPORT jint JNI_OnLoad(JavaVM* vm, void* /*reserved*/) {
    JNIEnv* env;
    jclass c;
    int rc;

    if (vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) != JNI_OK) {
        ALOGE("%s: Failed to get the environment", __FUNCTION__);
        return JNI_ERR;
    }

    c = env->FindClass("android/system/virtualmachine/VirtualMachine");
    if (c == nullptr) {
        ALOGE("%s: Failed to find class android.system.virtualmachine.VirtualMachine",
              __FUNCTION__);
        return JNI_ERR;
    }

    // Register your class' native methods.
    static const JNINativeMethod methodsVirtualMachine[] = {
            {"nativeConnectToVsockServer", "(Landroid/os/IBinder;I)Landroid/os/IBinder;",
             reinterpret_cast<void*>(
                     android_system_virtualmachine_VirtualMachine_connectToVsockServer)},
    };
    rc = env->RegisterNatives(c, methodsVirtualMachine,
                              sizeof(methodsVirtualMachine) / sizeof(JNINativeMethod));
    if (rc != JNI_OK) {
        ALOGE("%s: Failed to register natives", __FUNCTION__);
        return rc;
    }

    c = env->FindClass("android/system/virtualmachine/VirtualizationService");
    if (c == nullptr) {
        ALOGE("%s: Failed to find class android.system.virtualmachine.VirtualizationService",
              __FUNCTION__);
        return JNI_ERR;
    }

    sVirtualizationServiceCtor = env->GetMethodID(c, "<init>", "(I)V");
    if (sVirtualizationServiceCtor == nullptr) {
        ALOGE("%s: Failed to find constructor of class "
              "android.system.virtualmachine.VirtualizationService",
              __FUNCTION__);
        return JNI_ERR;
    }

    // Register your class' native methods.
    static const JNINativeMethod methodsVirtualizationService[] = {
            {"nativeSpawn", "()Landroid/system/virtualmachine/VirtualizationService;",
             reinterpret_cast<void*>(android_system_virtualmachine_VirtualizationService_spawn)},
            {"nativeConnect", "(I)Landroid/os/IBinder;",
             reinterpret_cast<void*>(android_system_virtualmachine_VirtualizationService_connect)},
            {"nativeFinalize", "(I)V",
             reinterpret_cast<void*>(android_system_virtualmachine_VirtualizationService_finalize)},
    };
    rc = env->RegisterNatives(c, methodsVirtualizationService,
                              sizeof(methodsVirtualizationService) / sizeof(JNINativeMethod));
    if (rc != JNI_OK) {
        ALOGE("%s: Failed to register natives", __FUNCTION__);
        return rc;
    }

    return JNI_VERSION_1_6;
}
