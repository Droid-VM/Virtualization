/*
 * Copyright (C) 2024 The Android Open Source Project
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

package com.android.virtualization.vmlauncher;

import android.app.Activity;
import android.os.Bundle;
import android.system.virtualmachine.VirtualMachine;
import android.system.virtualmachine.VirtualMachineConfig;
import android.system.virtualmachine.VirtualMachineException;
import android.util.Log;

import java.nio.file.Path;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public class VmLauncherActivity extends Activity {
    static final String TAG = "VmLauncherApp";
    // TODO: this path should be from outside of this activity
    private static final String VM_CONFIG_PATH = "/data/local/tmp/vm_config.json";

    private static final int RECORD_AUDIO_PERMISSION_REQUEST_CODE = 101;

    private static final String ACTION_VM_LAUNCHER = "android.virtualization.VM_LAUNCHER";
    private static final String ACTION_VM_OPEN_URL = "android.virtualization.VM_OPEN_URL";

    protected ExecutorService mExecutorService;
    protected VirtualMachine mVirtualMachine;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        if (!setupBeforeVmCreate()) {
            return;
        }
        mExecutorService = Executors.newCachedThreadPool();

        ConfigJson json = ConfigJson.from(VM_CONFIG_PATH);
        VirtualMachineConfig config = json.toConfig(this);

        Runner runner;
        try {
            runner = Runner.create(this, config);
        } catch (VirtualMachineException e) {
            throw new RuntimeException(e);
        }
        mVirtualMachine = runner.getVm();
        runner.getExitStatus()
                .thenAcceptAsync(
                        success -> {
                            setResult(success ? RESULT_OK : RESULT_CANCELED);
                            finish();
                        });

        Path logPath = getFileStreamPath(mVirtualMachine.getName() + ".log").toPath();
        Logger.setup(mVirtualMachine, logPath, mExecutorService);
    }

    protected boolean setupBeforeVmCreate() {
        return true;
    }

    protected boolean wantSuspendInBackground() {
        return true;
    }

    @Override
    protected void onStop() {
        super.onStop();
        if (wantSuspendInBackground()) {
            try {
                mVirtualMachine.suspend();
            } catch (VirtualMachineException e) {
                Log.e(TAG, "Failed to suspend VM" + e);
            }
        }
    }

    @Override
    protected void onRestart() {
        super.onRestart();

        if (wantSuspendInBackground()) {
            try {
                mVirtualMachine.resume();
            } catch (VirtualMachineException e) {
                Log.e(TAG, "Failed to resume VM" + e);
            }
        }
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        mExecutorService.shutdownNow();
    }
}
