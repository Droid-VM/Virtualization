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

package com.android.microdroid.testservice;

/** {@hide} */
interface IReportService {
    const int SERVICE_PORT = 5677;
    const String SEQ_READ_MEAN = "seq_read_mean";
    const String SEQ_READ_STD = "seq_read_std";
    const String RAND_READ_MEAN = "rand_read_mean";
    const String RAND_READ_STD = "rand_read_std";

    /** Gets the double value of the given key. */
    double getDouble(String key);
}
