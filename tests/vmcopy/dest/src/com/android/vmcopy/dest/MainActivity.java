/*
 * Copyright (C) 2022 The Android Open Source Project
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

package com.android.vmcopy.dest;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;
import android.os.IBinder;
import android.os.ParcelFileDescriptor;
import android.os.RemoteException;
import android.system.virtualmachine.VirtualMachine;
import android.system.virtualmachine.VirtualMachineCallback;
import android.system.virtualmachine.VirtualMachineDescriptor;
import android.system.virtualmachine.VirtualMachineException;
import android.system.virtualmachine.VirtualMachineManager;
import android.util.Log;

import androidx.appcompat.app.AppCompatActivity;
import androidx.lifecycle.MutableLiveData;

import com.android.microdroid.testservice.ITestService;

import java.util.concurrent.Executors;

public class MainActivity extends AppCompatActivity {
    private static final String TAG = "VmCopyDest";
    private static final String VM_NAME = "vm_dest";
    private static final String VM_DESCRIPTOR_KEY = "vm_descriptor";

    private final MutableLiveData<Boolean> mIsFinished = new MutableLiveData<>();
    private VirtualMachine mVirtualMachine;

    private class DestCallback implements VirtualMachineCallback {
        @Override
        public void onPayloadStarted(VirtualMachine vm) {}

        @Override
        public void onPayloadReady(VirtualMachine vm) {
            IBinder binder;
            try {
                binder = vm.connectToVsockServer(ITestService.SERVICE_PORT);
            } catch (Exception e) {
                if (!Thread.interrupted()) {
                    Log.i(TAG, "VM service connection failed:" + e.getMessage());
                }
                return;
            }

            try {
                ITestService testService = ITestService.Stub.asInterface(binder);
                int ret = testService.addInteger(123, 456);
                Intent result = new Intent();
                setResult(Activity.RESULT_OK, result);
                Log.i(TAG, "VM payload service: 123 + 456 = " + ret);
                mIsFinished.postValue(true);
            } catch (RemoteException e) {
                Log.i(TAG, "Exception while testing VM's binder service: " + e.getMessage());
            }
        }

        @Override
        public void onPayloadFinished(VirtualMachine vm, int exitCode) {
            Log.i(TAG, "Payload finished. exit code:" + exitCode);
        }

        @Override
        public void onError(VirtualMachine vm, int errorCode, String message) {
            Log.i(TAG, "Error occurred. code:" + errorCode + ", message:" + message);
        }

        @Override
        public void onStopped(VirtualMachine vm, int reason) {
            Log.i(TAG, "Vm stopped");
        }

        @Override
        public void onRamdump(VirtualMachine vm, ParcelFileDescriptor ramdump) {
            Log.e(TAG, "Kernel panic. Ramdump created");
        }
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        VirtualMachineCallback callback = new DestCallback();
        mIsFinished.observeForever(
                isFinished -> {
                    if (isFinished && mVirtualMachine != null) {
                        try {
                            mVirtualMachine.stop();
                        } catch (VirtualMachineException e) {
                            // Consume
                        }
                        mVirtualMachine = null;
                    }
                });
        try {
            VirtualMachineManager vmm =
                    getApplication().getSystemService(VirtualMachineManager.class);
            VirtualMachineDescriptor vmDescriptor =
                    getIntent().getExtras().getParcelable(VM_DESCRIPTOR_KEY);
            mVirtualMachine = vmm.importFromDescriptor(VM_NAME, vmDescriptor);
            mVirtualMachine.run();
            mVirtualMachine.setCallback(Executors.newSingleThreadExecutor(), callback);
        } catch (VirtualMachineException e) {
            throw new RuntimeException(e);
        }
    }

    private void stopVm() {
        try {
            mVirtualMachine.stop();
        } catch (VirtualMachineException e) {
            // Consume
        }
        mVirtualMachine = null;
    }
}
