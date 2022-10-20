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

#include "common.h"

#include <log/log.h>

JNIEXPORT jint JNI_OnLoad(JavaVM* vm, void* /*reserved*/) {
    JNIEnv* env;
    jint rc;

    if (vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) != JNI_OK) {
        ALOGE("%s: Failed to get the environment", __FUNCTION__);
        return JNI_ERR;
    }

    if (JNI_register_android_system_virtualmachine_VirtualizationService(env) != JNI_OK ||
        JNI_register_android_system_virtualmachine_VirtualMachine(env) != JNI_OK) {
        return JNI_ERR;
    }

    return JNI_VERSION_1_6;
}
