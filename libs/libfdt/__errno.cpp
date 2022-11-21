#include <errno.h>

int* __errno() {
    static int ___errno = 0;
    return &___errno;
}
