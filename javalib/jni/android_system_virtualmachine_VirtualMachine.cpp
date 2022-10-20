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

JNIEXPORT jobject JNICALL android_system_virtualmachine_VirtualMachine_spawnVirtMgr(
        JNIEnv* env, [[maybe_unused]] jclass clazz) {
    using aidl::android::system::virtualizationservice::IVirtualizationService;
    using ndk::ScopedFileDescriptor;
    using ndk::SpAIBinder;

    unique_fd serverFd, clientFd, waitFd, signalFd;
    if (!Socketpair(AF_UNIX, &serverFd, &clientFd) || !Pipe(&waitFd, &signalFd, /*flags*/ 0)) {
        env->ThrowNew(env->FindClass("android/system/virtualmachine/VirtualMachineException"),
                      "Failed to create socketpair/pipe");
        return nullptr;
    }

    if (fork() == 0) {
        clientFd.reset();
        waitFd.reset();

        auto strServerFd = std::to_string(serverFd.get());
        auto strSignalFd = std::to_string(signalFd.get());

        execl("/apex/com.android.virt/bin/virtmgr", "/apex/com.android.virt/bin/virtmgr",
              "--rpc-server-fd", strServerFd.c_str(), "--ready-fd", strSignalFd.c_str(), NULL);
    }

    serverFd.reset();
    signalFd.reset();

    char buf;
    if (read(waitFd.get(), &buf, sizeof(buf)) < 0) {
        env->ThrowNew(env->FindClass("android/system/virtualmachine/VirtualMachineException"),
                      "Failed while waiting for readiness signal");
        return nullptr;
    }

    auto session = ARpcSession_new();
    // SAFETY - ARpcSession_setupUnixDomainBootstrapClient takes ownership of clientFd.
    auto client = ARpcSession_setupUnixDomainBootstrapClient(session, clientFd.release());
    auto obj = AIBinder_toJavaBinder(env, client);
    ARpcSession_free(session);
    return obj;
}

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

    auto session = ARpcSession_new();
    auto client = ARpcSession_setupPreconnectedClient(session, requestFunc, &args);
    auto obj = AIBinder_toJavaBinder(env, client);
    // Free the NDK handle. The underlying RpcSession object remains alive.
    ARpcSession_free(session);
    return obj;
}

JNIEXPORT jint JNI_OnLoad(JavaVM* vm, void* /*reserved*/) {
    JNIEnv* env;
    if (vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) != JNI_OK) {
        ALOGE("%s: Failed to get the environment", __FUNCTION__);
        return JNI_ERR;
    }

    jclass c = env->FindClass("android/system/virtualmachine/VirtualMachine");
    if (c == nullptr) {
        ALOGE("%s: Failed to find class android.system.virtualmachine.VirtualMachine",
              __FUNCTION__);
        return JNI_ERR;
    }

    // Register your class' native methods.
    static const JNINativeMethod methods[] = {
            {"nativeConnectToVsockServer", "(Landroid/os/IBinder;I)Landroid/os/IBinder;",
             reinterpret_cast<void*>(
                     android_system_virtualmachine_VirtualMachine_connectToVsockServer)},
            {"nativeSpawnVirtMgr", "()Landroid/os/IBinder;",
             reinterpret_cast<void*>(android_system_virtualmachine_VirtualMachine_spawnVirtMgr)},
    };
    int rc = env->RegisterNatives(c, methods, sizeof(methods) / sizeof(JNINativeMethod));
    if (rc != JNI_OK) {
        ALOGE("%s: Failed to register natives", __FUNCTION__);
        return rc;
    }

    return JNI_VERSION_1_6;
}
