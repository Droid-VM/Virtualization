#include <stdio.h>
#include <vm_main.h>

extern "C" int AVmPayload_main() {
    // disable buffering to communicate seamlessly
    setvbuf(stdin, nullptr, _IONBF, 0);
    setvbuf(stdout, nullptr, _IONBF, 0);
    setvbuf(stderr, nullptr, _IONBF, 0);

    printf("Hello Microdroid\n");

    return 0;
}
