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

#include <chrono>

using namespace android::base;

Result<double> send_data(int fd, const char *filename) {
    sleep(3);
    std::string data;
    unique_fd fd_data(open(filename, O_RDONLY | O_CLOEXEC));
    if (!ReadFdToString(fd_data, &data)) {
        return Error() << "Cannot read data to string";
    }
    struct timespec ts;
    timespec_get(&ts, TIME_UTC);
    double send_time = ts.tv_sec + 1e-9 * ts.tv_nsec;
    LOG(INFO) << "Host:Sending data at '" << send_time << "'";
    if (!WriteStringToFd(data, fd)) {
        return Error() << "Cannot send data to client";
    }
    LOG(INFO) << "Host:Finished sending data...";
    return {send_time};
}

extern "C" JNIEXPORT jdouble JNICALL
Java_com_android_microdroid_benchmark_IoVsockHostNative_sendData(JNIEnv *env, __unused jclass clazz,
                                                                 int fd, jstring jfilename) {
    const char *filename = env->GetStringUTFChars(jfilename, NULL);
    if (auto res = send_data(fd, filename); res.ok()) {
        return res.value();
    } else {
        LOG(ERROR) << "Host:Failed to receive data: " << res.error();
        abort();
    }
}
