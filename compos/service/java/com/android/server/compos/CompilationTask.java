/*
 * Copyright 2021 The Android Open Source Project
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

package com.android.server.compos;

import android.annotation.SystemApi;
import android.os.RemoteException;
import android.system.composd.ICompilationTask;

/**
 * Represents an in-progress isolated compilation task running in a VM.
 *
 * @hide
 */
@SystemApi(client = SystemApi.Client.SYSTEM_SERVER)
public class CompilationTask {
    private final ICompilationTask mComposTask;
    private final ComposCallback mComposCallback;

    CompilationTask(ICompilationTask composTask, ComposCallback composCallback) {
        mComposTask = composTask;
        mComposCallback = composCallback;
    }

    /**
     * Cancel the task, causing it to end as soon as possible. Calling cancel on an already-ended
     * task has no effect. After {@code cancel} has completed no further callbacks will be delivered
     * to the corresponding {@link CompilationCallback}.
     */
    public void cancel() {
        try {
            mComposTask.cancel();
        } catch (RemoteException e) {
            throw e.rethrowAsRuntimeException();
        }
        mComposCallback.disable();
        mComposTask.asBinder().unlinkToDeath(mComposCallback, 0);
    }
}
