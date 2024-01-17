/*
 * Copyright (C) 2023 The Android Open Source Project
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

package com.android.virt.service_vm.client;

import static android.system.virtualmachine.VirtualMachineConfig.DEBUG_LEVEL_FULL;
import android.system.virtualmachine.VirtualMachine;
import android.system.virtualmachine.VirtualMachineConfig;

import java.io.IOException;

import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.junit.runners.BlockJUnit4ClassRunner;

import com.android.microdroid.test.device.MicrodroidDeviceTestBase;

@RunWith(BlockJUnit4ClassRunner.class)
public class AttestationClientVmTests extends MicrodroidDeviceTestBase {
    private static final String TAG = "AttestationClientVm";
    private static final String DEFAULT_CONFIG = "assets/config.json";
    private static final long MEMORY_MIB = 256;

    @Before
    public void setup() throws IOException {
        grantPermission(VirtualMachine.MANAGE_VIRTUAL_MACHINE_PERMISSION);
        grantPermission(VirtualMachine.USE_CUSTOM_VIRTUAL_MACHINE_PERMISSION);
        prepareTestSetup(true /* protectedVm */, null /* gki */);
        setMaxPerformanceTaskProfile();
    }

    @Test
    public void runAttestationClient() throws Exception {
        VirtualMachineConfig.Builder builder =
                newVmConfigBuilderWithPayloadConfig(DEFAULT_CONFIG)
                        .setMemoryBytes(MEMORY_MIB * 1024 * 1024)
                        .setDebugLevel(DEBUG_LEVEL_FULL)
                        .setVmOutputCaptured(true);

        VirtualMachineConfig config = builder.build();
        VirtualMachine vm = forceCreateNewVirtualMachine("attestation_client", config);
        android.os.Trace.beginSection("runRequestAttestationInClientVm");
        runVmTestService(TAG, vm, (ts, tr) -> {});
        android.os.Trace.endSection();
    }
}
