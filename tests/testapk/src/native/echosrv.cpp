#include <android-base/file.h>
#include <android-base/properties.h>
#include <android-base/result.h>
#include <android-base/scopeguard.h>
#include <android/log.h>
#include <linux/vm_sockets.h>
#include <unistd.h>

#include <cstdint>
#include <string>
#include <thread>

using android::base::borrowed_fd;
using android::base::ErrnoError;
using android::base::Error;
using android::base::make_scope_guard;
using android::base::Result;
using android::base::unique_fd;

constexpr char TAG[] = "echosrv";
constexpr uint32_t ECHO_REVERSE_PORT = 0x80000001U;

Result<void> run_echo_reverse_server(borrowed_fd listening_fd) {
    struct sockaddr_vm client_sa = {};
    socklen_t client_sa_len = sizeof(client_sa);
    unique_fd connect_fd{accept4(listening_fd.get(), (struct sockaddr*)&client_sa, &client_sa_len,
                                 SOCK_CLOEXEC)};
    if (!connect_fd.ok()) {
        return ErrnoError() << "Failed to accept vsock connection";
    }

    unique_fd input_fd{fcntl(connect_fd, F_DUPFD_CLOEXEC, 0)};
    if (!input_fd.ok()) {
        return ErrnoError() << "Failed to dup";
    }
    FILE* input = fdopen(input_fd.release(), "r");
    if (!input) {
        return ErrnoError() << "Failed to fdopen";
    }

    // Run forever, reverse one line at a time.
    while (true) {
        char* line = nullptr;
        size_t size = 0;
        if (getline(&line, &size, input) < 0) {
            if (errno == 0) {
                return {}; // the input was closed
            }
            return ErrnoError() << "Failed to read";
        }

        std::string_view original = line;
        if (!original.empty() && original.back() == '\n') {
            original = original.substr(0, original.size() - 1);
        }

        std::string reversed(original.rbegin(), original.rend());
        reversed += "\n";

        if (write(connect_fd, reversed.data(), reversed.size()) < 0) {
            return ErrnoError() << "Failed to write";
        }
    }
}

Result<void> start_echo_reverse_server() {
    unique_fd server_fd{TEMP_FAILURE_RETRY(socket(AF_VSOCK, SOCK_STREAM | SOCK_CLOEXEC, 0))};
    if (!server_fd.ok()) {
        return ErrnoError() << "Failed to create vsock socket";
    }
    struct sockaddr_vm server_sa = (struct sockaddr_vm){
            .svm_family = AF_VSOCK,
            .svm_port = static_cast<uint32_t>(ECHO_REVERSE_PORT),
            .svm_cid = VMADDR_CID_ANY,
    };
    int ret = TEMP_FAILURE_RETRY(bind(server_fd, (struct sockaddr*)&server_sa, sizeof(server_sa)));
    if (ret < 0) {
        return ErrnoError() << "Failed to bind vsock socket";
    }
    ret = TEMP_FAILURE_RETRY(listen(server_fd, /*backlog=*/1));
    if (ret < 0) {
        return ErrnoError() << "Failed to listen";
    }

    std::thread accept_thread{[listening_fd = std::move(server_fd)] {
        Result<void> result;
        while ((result = run_echo_reverse_server(listening_fd)).ok()) {
        }
        __android_log_write(ANDROID_LOG_ERROR, TAG, result.error().message().c_str());
        // Make sure the VM exits so the test will fail solidly
        exit(1);
    }};
    accept_thread.detach();

    return {};
}

int main(void) {
    auto ret = start_echo_reverse_server();

    if (!ret.ok()) {
        fprintf(stderr, "Failed to start echo server\n");
        return -1;
    }

    for (;;) {
        asm volatile("");
    }

    return 0;
}
