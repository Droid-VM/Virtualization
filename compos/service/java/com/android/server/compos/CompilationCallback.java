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

/**
 * Interface to be implemented by clients of {@link IsolatedCompilation} to be notified when a
 * {@link CompilationTask} completes.
 *
 * @hide
 */
@SystemApi(client = SystemApi.Client.SYSTEM_SERVER)
public interface CompilationCallback {
    /**
     * Called when a {@link CompilationTask} has ended successfully, generating all the required
     * artifacts.
     */
    void onSuccess();

    /** Called when a {@link CompilationTask} has ended unsuccessfully. */
    void onFailure();
}
