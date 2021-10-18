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
import android.os.IBinder;
import android.system.composd.ICompilationTaskCallback;

import java.util.Objects;
import java.util.concurrent.atomic.AtomicBoolean;

/** Handle binder callbacks from composd and convert them to Java callbacks to System Server. */
class ComposCallback extends ICompilationTaskCallback.Stub implements IBinder.DeathRecipient {
    private final CompilationCallback mClientCallback;
    private final AtomicBoolean mDisabled = new AtomicBoolean(false);

    ComposCallback(@NonNull CompilationCallback clientCallback) {
        mClientCallback = Objects.requireNonNull(clientCallback);
    }

    void disable() {
        mDisabled.set(true);
    }

    @Override
    public void onSuccess() {
        if (!mDisabled.getAndSet(true)) {
            mClientCallback.onSuccess();
        }
    }

    @Override
    public void onFailure() {
        if (!mDisabled.getAndSet(true)) {
            mClientCallback.onFailure();
        }
    }

    @Override
    public void binderDied() {
        onFailure();
    }
}
