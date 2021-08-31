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

package android.system.virtualmachine;

import android.annotation.NonNull;
import android.annotation.Nullable;
import android.os.IBinder;
import android.os.ParcelFileDescriptor;

/**
 * Callback interface to get notified with the events from the virtual machine. The methods are
 * executed on either a binder thread or an ExecutorService worker thread. Implementations can make
 * blocking calls in the methods.
 *
 * @hide
 */
public interface VirtualMachineCallback {

    /** Called when the payload starts in the VM. */
    void onPayloadStarted(@NonNull VirtualMachine vm, @Nullable ParcelFileDescriptor stream);

    /** Called when the payload in the VM is ready to serve. */
    void onPayloadReady(@NonNull VirtualMachine vm);

    /** Called when the payload has finished in the VM. */
    void onPayloadFinished(@NonNull VirtualMachine vm, int exitCode);

    /** Called when the requested vsock server is connected. */
    void onVsockServerReady(@NonNull VirtualMachine vm, int port, IBinder binder);

    /** Called when the connection to the requested vsock server fails. */
    void onVsockServerConnectionFailed(@NonNull VirtualMachine vm, int port, String error);

    /** Called when the VM died. */
    void onDied(@NonNull VirtualMachine vm);
}
