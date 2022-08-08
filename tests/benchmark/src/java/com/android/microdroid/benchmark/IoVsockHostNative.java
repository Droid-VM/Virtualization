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

package com.android.microdroid.benchmark;

class IoVsockHostNative {
    static {
        System.loadLibrary("iovsock_host_jni");
    }

    /**
     * Sends data from the host to a VM.
     * @return The transfer rate in MB/s for sending data from host to VM.
     */
    static native double sendData(int fd);
}
