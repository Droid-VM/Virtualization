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

package com.android.vmcopy.source;

import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.os.Bundle;
import android.os.IBinder;
import android.os.ParcelFileDescriptor;
import android.os.RemoteException;
import android.system.virtualmachine.VirtualMachine;
import android.system.virtualmachine.VirtualMachineCallback;
import android.system.virtualmachine.VirtualMachineConfig;
import android.system.virtualmachine.VirtualMachineException;
import android.system.virtualmachine.VirtualMachineManager;
import android.util.Log;

import androidx.activity.result.ActivityResult;
import androidx.activity.result.ActivityResultCallback;
import androidx.activity.result.ActivityResultLauncher;
import androidx.activity.result.contract.ActivityResultContracts.StartActivityForResult;
import androidx.appcompat.app.AppCompatActivity;
import androidx.lifecycle.MutableLiveData;

import com.android.microdroid.testservice.ITestService;

import java.util.concurrent.Executors;

public class MainActivity extends AppCompatActivity {
    private static final String TAG = "VmCopySource";
    private static final String DEST_APK_PKG = "com.android.vmcopy.source";
    private static final String VM_NAME = "vm_source";
    private static final String VM_DESCRIPTOR_KEY = "vm_descriptor";
    private static final String TEST_RESULT_KEY = "test_result";

    private final MutableLiveData<Boolean> mIsFinished = new MutableLiveData<>();
    private VirtualMachine mVirtualMachine;

    private static class TestResultCallback implements ActivityResultCallback<ActivityResult> {
        @Override
        public void onActivityResult(ActivityResult result) {
            if (result.getResultCode() != Activity.RESULT_OK) {
                System.exit(1);
            }
            Intent intent = result.getData();
            boolean isSuccess = intent.getBooleanExtra(TEST_RESULT_KEY, /*defaultValue=*/ false);
            if (!isSuccess) {
                System.exit(1);
            }
        }
    }

    private class SourceCallback implements VirtualMachineCallback {
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
        VirtualMachineCallback callback = new SourceCallback();
        ActivityResultLauncher<Intent> getTestResult =
                registerForActivityResult(new StartActivityForResult(), new TestResultCallback());
        mIsFinished.observeForever(
                isFinished -> {
                    if (isFinished) {
                        stopVm();
                        getTestResult.launch(newIntentToLaunchDestApp());
                    }
                });
        try {
            VirtualMachineConfig.Builder builder =
                    new VirtualMachineConfig.Builder(getApplication());
            builder.setPayloadBinaryPath("MicrodroidTestNativeLib.so");
            builder.setProtectedVm(true);
            builder.setDebugLevel(VirtualMachineConfig.DEBUG_LEVEL_FULL);
            VirtualMachineConfig config = builder.build();
            VirtualMachineManager vmm =
                    getApplication().getSystemService(VirtualMachineManager.class);
            mVirtualMachine = vmm.getOrCreate(VM_NAME, config);
            try {
                mVirtualMachine.setConfig(config);
            } catch (VirtualMachineException e) {
                vmm.delete(VM_NAME);
                mVirtualMachine = vmm.create(VM_NAME, config);
            }
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
            throw new RuntimeException("Cannot stop VM", e);
        }
    }

    private Intent newIntentToLaunchDestApp() {
        Intent intent = new Intent(Intent.ACTION_SEND);
        try {
            intent.putExtra(VM_DESCRIPTOR_KEY, mVirtualMachine.toDescriptor());
        } catch (VirtualMachineException e) {
            throw new RuntimeException("Cannot convert VM to descriptor", e);
        }
        intent.setComponent(new ComponentName(DEST_APK_PKG, DEST_APK_PKG + ".MainActivity"));
        return intent;
    }
}
