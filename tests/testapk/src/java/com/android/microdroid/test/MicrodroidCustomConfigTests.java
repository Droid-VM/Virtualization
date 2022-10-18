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
package com.android.microdroid.test;

import static android.system.virtualmachine.VirtualMachineConfig.DEBUG_LEVEL_FULL;

import static com.google.common.truth.Truth.assertThat;

import android.system.virtualmachine.VirtualMachine;
import android.system.virtualmachine.VirtualMachineCallback;
import android.system.virtualmachine.VirtualMachineConfig;

import com.android.compatibility.common.util.CddTest;

import org.junit.Test;
import org.junit.runner.RunWith;
import org.junit.runners.Parameterized;

@RunWith(Parameterized.class)
public class MicrodroidCustomConfigTests extends MicrodroidTestsBase {

    @Test
    @CddTest(requirements = {
            "9.17/C-1-1",
            "9.17/C-2-1"
    })
    public void extraApk() throws Exception {
        assumeSupportedKernel();

        VirtualMachineConfig config = mInner.newVmConfigBuilder()
                .setPayloadConfigPath("assets/vm_config_extra_apk.json")
                .setMemoryMib(minMemoryRequired())
                .build();
        VirtualMachine vm = mInner.forceCreateNewVirtualMachine("test_vm_extra_apk", config);

        TestResults testResults = runVmTestService(vm);
        assertThat(testResults.mExtraApkTestProp).isEqualTo("PASS");
    }

    @Test
    public void bootFailsWhenConfigIsInvalid() throws Exception {
        VirtualMachineConfig normalConfig = mInner.newVmConfigBuilder()
                .setPayloadConfigPath("assets/vm_config_no_task.json")
                .setDebugLevel(DEBUG_LEVEL_FULL)
                .build();
        mInner.forceCreateNewVirtualMachine("test_vm_invalid_config", normalConfig);

        BootResult bootResult = tryBootVm(TAG, "test_vm_invalid_config");
        assertThat(bootResult.payloadStarted).isFalse();
        assertThat(bootResult.deathReason).isEqualTo(
                VirtualMachineCallback.STOP_REASON_MICRODROID_INVALID_PAYLOAD_CONFIG);
    }
}

