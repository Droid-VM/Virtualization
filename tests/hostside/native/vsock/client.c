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
    struct sockaddr_vm sa;
    unsigned int cid, port;
    int fd, ret;
    char buf[1024];
    ssize_t nbytes;

    if (argc != 3 || !parse_uint(argv[1], &cid) || !parse_uint(argv[2], &port)) {
        FATAL_ERROR("Usage: %s <cid> <port>\n", argv[0]);
    }

    sa = (struct sockaddr_vm) {
        .svm_family = AF_VSOCK,
        .svm_cid = cid,
        .svm_port = port,
    };

    fd = socket(AF_VSOCK, SOCK_STREAM, 0);
    if (fd < 0)
        FATAL_PERROR(socket);

    ret = connect(fd, (struct sockaddr*)&sa, sizeof(sa));
    if (ret < 0)
        FATAL_PERROR(connect);

    do {
        nbytes = read(fd, buf, sizeof(buf));
        if (nbytes < 0)
            FATAL_PERROR(read);
        fwrite(buf, sizeof(char), (size_t)nbytes, stdout);
    } while (nbytes > 0);

    close(fd);
    return EXIT_SUCCESS;
}
