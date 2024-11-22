#include <android/virtualization.h>
#include <fcntl.h>
#include <getopt.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#include <string>

int main(int argc, char** argv) {
    std::string kernel;
    bool protected_vm = false;
    std::string name = "trusty_security_vm_launcher";
    int memory_size_mib = 128;

    const option long_options[] = {{"kernel", required_argument, nullptr, 'k'},
                                   {"protected", no_argument, nullptr, 'p'},
                                   {"name", required_argument, nullptr, 'n'},
                                   {"memory-size-mib", required_argument, nullptr, 'm'},
                                   {nullptr, 0, nullptr, 0}};

    int option_index = 0;
    int c;

    while ((c = getopt_long_only(argc, argv, "k:pn:m:c:", long_options, &option_index)) != -1) {
        switch (c) {
            case 'k':
                kernel = optarg;
                break;
            case 'p':
                protected_vm = true;
                break;
            case 'n':
                name = optarg;
                break;
            case 'm':
                memory_size_mib = std::stoi(optarg);
                break;
            case '?':
                // getopt_long_only already prints an error message
                exit(1);
            default:
                exit(1);
        }
    }

    if (kernel.empty()) {
        fprintf(stderr, "Error: Missing required argument --kernel\n");
        exit(1);
    }

    AVirtualizationService* service;
    if (AVirtualizationService_create(&service, true /* early vm */) != 0) {
        fprintf(stderr, "create_early failed\n");
        exit(1);
    }

    int kernel_fd = open(kernel.c_str(), O_RDONLY);
    if (kernel_fd == -1) {
        fprintf(stderr, "opening kernel failed\n");
        exit(1);
    }

    AVirtualMachineConfig* config = AVirtualMachineConfig_createRaw();
    AVirtualMachineConfig_setName(config, name.c_str());
    AVirtualMachineConfig_setKernel(config, kernel_fd);
    AVirtualMachineConfig_setProtectedVm(config, protected_vm);
    AVirtualMachineConfig_setMemoryMib(config, memory_size_mib);

    printf("creating VM with LLNDK\n");

    AVirtualMachine* vm;
    if (AVirtualMachine_create(service, config,
                               // console_in, console_out, and log will be redirected to the kernel
                               // log by virtmgr
                               -1, // console_in
                               -1, // console_out
                               -1, // log
                               &vm) != 0) {
        fprintf(stderr, "creating VM failed\n");
        exit(1);
    }

    if (AVirtualMachine_start(vm) != 0) {
        fprintf(stderr, "starting VM failed\n");
        exit(1);
    }

    printf("started trusty_security_vm_launcher VM\n");
    auto death_reason = AVirtualMachine_waitForStop(vm);
    fprintf(stderr, "trusty_security_vm_launcher ended: %d\n", death_reason);
}
