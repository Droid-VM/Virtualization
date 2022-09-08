// Copyright 2022, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
#include <stddef.h>

#define __weak __attribute__((__weak__))

__weak void *memchr(const void *src, int c, size_t n) {
    return __builtin_memchr(src, c, n);
}

__weak size_t strlen(const char *s) {
    return __builtin_strlen(s);
}

__weak char *strrchr(const char *s, int c) {
    return __builtin_strrchr(s, c);
}
