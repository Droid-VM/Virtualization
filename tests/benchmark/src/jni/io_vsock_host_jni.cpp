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

#include <android-base/file.h>
#include <android-base/logging.h>
#include <android-base/result.h>
#include <android-base/unique_fd.h>
#include <jni.h>
#include <linux/vm_sockets.h> // Needs to be included after sys/socket.h
#include <sys/socket.h>

using namespace android::base;

Result<void> connect_socket(int fd, [[maybe_unused]] unsigned int port) {
    LOG(INFO) << "Host:Receiving data...";
    std::string message = "HelloHelloHelloFromHost";
    if (ReadFdToString(fd, &message)) {
        LOG(ERROR) << "Host:" << message;
    } else {
        return Error() << "Cannot read data";
    }
    LOG(INFO) << "Host:Sending data...";
    if (!WriteStringToFd(message, fd)) {
        return Error() << "Cannot send message to client";
    }
    return {};
}

extern "C" JNIEXPORT jdouble JNICALL
Java_com_android_microdroid_benchmark_IoVsockHostNative_sendDataFromHostToVM(JNIEnv *, jclass,
                                                                             int fd, int port) {
    if (auto res = connect_socket(fd, (unsigned int)port); !res.ok()) {
        LOG(ERROR) << "Host:Failed to connect socket: " << res.error();
        return -1;
    }
    double result = 100.0;
    return result;
}
