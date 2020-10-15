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

#include <errno.h>
#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#define FATAL_ERROR(MSG, ...)                             \
    do {                                                  \
        fprintf(stderr, "ERROR: " MSG "\n", __VA_ARGS__); \
        exit(EXIT_FAILURE);                               \
    } while(0)

#define FATAL_PERROR(CALL)  \
    do {                    \
        perror(#CALL);      \
        exit(EXIT_FAILURE); \
    } while(0)

static bool parse_uint(const char *str, unsigned int *out) {
    unsigned long int ret;

    ret = strtoul(str, NULL, /* base */ 10);
    if (ret == ULONG_MAX) {
        return false;
    }

    *out = (unsigned int)ret;
    return true;
}
