#include <android-base/logging.h>
#include <android-base/result.h>
#include <stdio.h>

#include "io_vsock.h"

int main(int argc, char** argv) {
    android::base::SetLogger(android::base::StderrLogger);

    if (argc < 3) {
        fprintf(stderr, "Usage: vsock_server <port> <bytes to receive>\n");
        return 1;
    }

    int port = atoi(argv[1]);
    int bytes_to_receive = atoi(argv[2]);

    auto server_fd = io_vsock::init_vsock_server(port);
    if (!server_fd.ok()) {
        LOG(ERROR) << "Failed to start vsock server : " << server_fd.error();
        return 1;
    }

    auto result = io_vsock::run_vsock_server_and_receive_data(*server_fd, bytes_to_receive);
    if (!result.ok()) {
        LOG(ERROR) << "Failed to run benchmark : " << result.error();
    }

    return 0;
}
