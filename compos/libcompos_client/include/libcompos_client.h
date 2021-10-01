/*
 * Copyright (C) 2021 The Android Open Source Project
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

#pragma once

#include <stdint.h>
#include <sys/cdefs.h>

__BEGIN_DECLS

/**
 * Sends request encoded in a marshaled byte buffer to the Compilation OS service.
 *
 * Available since API level 33.
 */
int AComposClient_Request(int cid, const uint8_t* marshaled, size_t size, const int* ro_fds,
                          size_t ro_fds_num, const int* rw_fds, size_t rw_fds_num)
        __INTRODUCED_IN(33);

__END_DECLS
