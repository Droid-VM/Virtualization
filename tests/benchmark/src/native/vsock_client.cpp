#include <android-base/file.h>
#include <android-base/logging.h>
#include <android-base/result.h>
#include <linux/vm_sockets.h>
#include <stdio.h>
#include <sys/socket.h>
#include <time.h>

#include <iostream>

using android::base::ErrnoError;
using android::base::Error;
using android::base::Result;
using android::base::WriteStringToFd;

constexpr size_t kNumBytesPerMB = 1024 * 1024;

Result<double> measure_send_rate(int fd, int num_bytes_to_send) {
    std::string data;
    data.assign(num_bytes_to_send, 'a');
    struct timespec start;
    if (clock_gettime(CLOCK_MONOTONIC, &start) == -1) {
        return ErrnoError() << "failed to clock_gettime";
    }
    if (!WriteStringToFd(data, fd)) {
        return Error() << "Cannot send data to client";
    }
    struct timespec finish;
    if (clock_gettime(CLOCK_MONOTONIC, &finish) == -1) {
        return ErrnoError() << "failed to clock_gettime";
    }
    double elapsed_seconds = finish.tv_sec - start.tv_sec + (finish.tv_nsec - start.tv_nsec) / 1e9;
    LOG(INFO) << "Host:Finished sending data in " << elapsed_seconds << " seconds.";
    double send_rate = (double)num_bytes_to_send / kNumBytesPerMB / elapsed_seconds;
    return {send_rate};
}

int main(int argc, char** argv) {
    android::base::SetLogger(android::base::StderrLogger);

    if (argc < 4) {
        fprintf(stderr, "Usage: vsock_client <CID> <port> <bytes_to_send>\n");
        return 1;
    }

    unsigned int vm_cid = atoi(argv[1]);
    unsigned int port = atoi(argv[2]);
    int bytes_to_send = atoi(argv[3]);

    int socket_fd = socket(AF_VSOCK, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (socket_fd < 0) {
        PLOG(ERROR) << "Failed to create socket";
        return 1;
    }
    struct sockaddr_vm vm_sa = (struct sockaddr_vm){
            .svm_family = AF_VSOCK,
            .svm_port = port,
            .svm_cid = vm_cid,
    };

    if (connect(socket_fd, (struct sockaddr*)&vm_sa, sizeof(vm_sa)) < 0) {
        PLOG(ERROR) << "Failed to connect to VM with CID : " << vm_cid << " on port " << port;
        return 1;
    }

    auto result = measure_send_rate(socket_fd, bytes_to_send);
    if (!result.ok()) {
        LOG(ERROR) << "Failed to run vsock benchmark : " << result.error();
        return 1;
    }

    LOG(INFO) << "[vsock-bench] " << *result;
    std::cout << *result << std::endl;
    return 0;
}
