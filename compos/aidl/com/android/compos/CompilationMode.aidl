/*
 * Copyright 2022 The Android Open Source Project
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

package com.android.compos;

/**
 * What type of compilation to perform.
 */
@Backing(type="int")
enum CompilationMode {
    /** Compile artifacts required by the current set of APEXes for use on reboot. */
    NORMAL_COMPILE = 0,
    /** Compile a full set of artifacts for test purposes. */
    TEST_COMPILE = 1,
}
