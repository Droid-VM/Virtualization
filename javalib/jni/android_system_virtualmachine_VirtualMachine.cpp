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

#include <log/log.h>

#include <android/binder_ibinder_jni.h>
#include <binder_rpc_unstable.hpp>

#include <jni.h>

JNIEXPORT jobject JNICALL android_system_virtualmachine_VirtualMachine_connectToVsockBinderServer(
        JNIEnv* env, [[maybe_unused]] jclass clazz, jint cid, jint port) {
    return AIBinder_toJavaBinder(env, RpcClient(cid, port));
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
            {"connectToVsockBinderServer", "(II)Landroid/os/IBinder;",
             reinterpret_cast<void*>(
                     android_system_virtualmachine_VirtualMachine_connectToVsockBinderServer)},
    };
    int rc = env->RegisterNatives(c, methods, sizeof(methods) / sizeof(JNINativeMethod));
    if (rc != JNI_OK) {
        ALOGE("%s: Failed to register natives", __FUNCTION__);
        return rc;
    }

    return JNI_VERSION_1_6;
}
