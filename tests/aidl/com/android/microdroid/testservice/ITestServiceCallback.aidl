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

/**
 * An object which a client may register with the VirtualizationService to get callbacks about the
 * state of a particular VM.
 */
interface ITestServiceCallback {
    /**
     * Called when the payload starts in the VM. `stream` is the input/output port of the payload.
     *
     * <p>Note: when the virtual machine object is shared to multiple processes and they register
     * this callback to the same virtual machine object, the processes will compete to access the
     * same payload stream. Keep only one process to access the stream.
     */
    void onTrigger(int param);
}
