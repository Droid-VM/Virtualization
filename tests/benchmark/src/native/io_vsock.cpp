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

#include <sys/socket.h>

// Needs to be included after sys/socket.h
#include <android-base/file.h>
#include <android-base/logging.h>
#include <android-base/result.h>
#include <android-base/unique_fd.h>
#include <linux/vm_sockets.h>

#include "io_vsock.h"

using namespace android::base;

namespace io_vsock {
Result<int> init_vsock_server(unsigned int port) {
    unique_fd server_fd(TEMP_FAILURE_RETRY(socket(AF_VSOCK, SOCK_STREAM, 0)));
    if (server_fd < 0) {
        return Error() << "VM:cannot create socket";
    }
    struct sockaddr_vm server_sa = (struct sockaddr_vm){
            .svm_family = AF_VSOCK,
            .svm_port = port,
            .svm_cid = VMADDR_CID_ANY,
    };

    LOG(INFO) << "VM:Connecting on port " << port << "...";
    int ret = TEMP_FAILURE_RETRY(bind(server_fd, (struct sockaddr *)&server_sa, sizeof(server_sa)));
    if (ret < 0) {
        return Error() << "VM:cannot bind an address with the socket";
    }
    ret = TEMP_FAILURE_RETRY(listen(server_fd, /*backlog=*/1));
    if (ret < 0) {
        return Error() << "VM:cannot listen to port";
    }
    LOG(INFO) << "Socket now listening";
    return {server_fd.get()};
}

Result<void> recv_data([[maybe_unused]] unsigned int cid, [[maybe_unused]] unsigned int port,
                       [[maybe_unused]] int server_fd) {
    LOG(INFO) << "VM:Accepting connection...";
    struct sockaddr_vm client_sa;
    socklen_t client_sa_len = sizeof(client_sa);
    unique_fd client_fd(
            TEMP_FAILURE_RETRY(accept(server_fd, (struct sockaddr *)&client_sa, &client_sa_len)));
    if (client_fd < 0) {
        return Error() << "Cannot retrieve connect requests";
    }

    LOG(INFO) << "VM:Receiving data from the host...";
    std::string message;
    if (ReadFdToString(client_fd, &message)) {
        LOG(ERROR) << "VM:" << message;
    } else {
        return Error() << "Cannot read data";
    }

    LOG(INFO) << "VM:Sending data to the host...";
    std::string message1 = "HelloHelloHelloFromVM";
    if (!WriteStringToFd(message1, client_fd)) {
        return Error() << "Cannot send data to host";
    }
    return {};
}
} // namespace io_vsock