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

import android.annotation.NonNull;
import android.annotation.SuppressLint;
import android.annotation.SystemApi;
import android.os.IBinder;
import android.os.RemoteException;
import android.os.ServiceManager;
import android.system.composd.ICompilationTask;
import android.system.composd.IIsolatedCompilationService;

/**
 * Exposes the ability to run isolated compilation, i.e. perform boot & system server classpath
 * compilation in a proected VM.
 *
 * @hide
 */
@SystemApi(client = SystemApi.Client.SYSTEM_SERVER)
public class IsolatedCompilation {

    /** TODO: Javadoc */
    @SuppressLint("ExecutorRegistration") // Client is system server, we never need an executor
    public @NonNull CompilationTask startStagedApexCompile(@NonNull CompilationCallback callback) {
        ComposCallback composCallback = new ComposCallback(callback);

        IBinder binder = ServiceManager.waitForService("android.system.composd");
        IIsolatedCompilationService composd = IIsolatedCompilationService.Stub.asInterface(binder);

        if (composd == null) {
            throw new IllegalStateException("Unable to find composd service");
        }

        try {
            // TODO: Move from test compile to real
            ICompilationTask composTask = composd.startTestCompile(composCallback);
            composTask.asBinder().linkToDeath(composCallback, 0);
            return new CompilationTask(composTask, composCallback);
        } catch (RemoteException e) {
            throw e.rethrowAsRuntimeException();
        }
    }
}
