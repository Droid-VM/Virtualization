/*
 * Copyright (C) 2020 The Android Open Source Project
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
#include <linux/vm_sockets.h>

#include "common.h"

int main(int argc, const char *argv[]) {
    struct sockaddr_vm server_sa, client_sa;
    unsigned int port;
    int server_fd, client_fd, ret;
    socklen_t client_sa_len = sizeof(client_sa);
    const char *msg;
    size_t msg_len, sent;
    ssize_t nbytes;

    if (argc != 3 || !parse_uint(argv[1], &port)) {
        FATAL_ERROR("Usage: %s <port> <message>", argv[0]);
    }

    msg = argv[2];
    msg_len = strlen(msg);

    server_sa = (struct sockaddr_vm) {
        .svm_family = AF_VSOCK,
        .svm_cid = VMADDR_CID_ANY,
        .svm_port = port,
    };

    server_fd = socket(AF_VSOCK, SOCK_STREAM, 0);
    if (server_fd < 0)
        FATAL_PERROR(socket);

    ret = bind(server_fd, (struct sockaddr*)&server_sa, sizeof(server_sa));
    if (ret != 0)
        FATAL_PERROR(bind);

    fprintf(stderr, "vsock_server: Listening on port %u...\n", port);
    ret = listen(server_fd, 1);
    if (ret != 0)
        FATAL_PERROR(listen);

    client_fd = accept(server_fd, (struct sockaddr*)&client_sa, &client_sa_len);
    if (client_fd < 0)
        FATAL_PERROR(accept);

    fprintf(stderr, "vsock_server: Sending test message to client...\n");
    for (sent = 0; sent < msg_len;) {
        nbytes = write(client_fd, msg + sent, msg_len - sent);
        if (nbytes > 0) {
            sent += nbytes;
        } else if (nbytes < 0 && errno != EAGAIN) {
            FATAL_PERROR(write);
        }
    }

    fprintf(stderr, "vsock_server: Exiting...\n");
    close(server_fd);
    close(client_fd);
    return EXIT_SUCCESS;
}
