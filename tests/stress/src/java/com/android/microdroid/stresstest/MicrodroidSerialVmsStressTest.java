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

package com.android.microdroid.stresstest;

import static android.system.virtualmachine.VirtualMachineConfig.DEBUG_LEVEL_NONE;

import static com.google.common.truth.Truth.assertThat;
import static com.google.common.truth.TruthJUnit.assume;

import android.system.virtualmachine.VirtualMachine;

import com.android.microdroid.test.device.MicrodroidDeviceTestBase;

import org.junit.Before;
import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.Timeout;
import org.junit.runner.RunWith;
import org.junit.runners.Parameterized;

import java.io.IOException;
import java.util.stream.IntStream;

@RunWith(Parameterized.class)
public class MicrodroidSerialVmsStressTest extends MicrodroidDeviceTestBase {
    private static final String TAG = "MicrodroidStressTests";
    private static final long ONE_MEBI = 1024 * 1024;
    private static final long MEM_SIZE = 80 * ONE_MEBI;

    @Rule public Timeout globalTimeout = Timeout.seconds(900);

    @Parameterized.Parameters(name = "protectedVm={0}")
    public static Object[] protectedVmConfigs() {
        return new Object[] {false, true};
    }

    @Parameterized.Parameters(name = "id={0}")
    public static Object[] testIdConfigs() {
        return IntStream.range(1, 255).mapToObj(Integer::new).toArray();
    }

    @Parameterized.Parameter public boolean mProtectedVm;
    @Parameterized.Parameter public int mTestId;

    @Before
    public void setup() throws IOException {
        assume().withMessage("Skip on CF; too slow").that(isCuttlefish()).isFalse();
        grantPermission(VirtualMachine.MANAGE_VIRTUAL_MACHINE_PERMISSION);
        grantPermission(VirtualMachine.USE_CUSTOM_VIRTUAL_MACHINE_PERMISSION);
        prepareTestSetup(mProtectedVm);
    }

    @Test
    public void testStartVm() throws Exception {
        String vmName = "stress_test_vm_" + mTestId;
        forceCreateNewVirtualMachine(
                vmName,
                newVmConfigBuilder()
                        .setPayloadBinaryName("MicrodroidIdleNativeLib.so")
                        .setMemoryBytes(MEM_SIZE)
                        .setDebugLevel(DEBUG_LEVEL_NONE)
                        .build());
        BootResult result = tryBootVm(TAG, vmName);
        forceDropVirtualMachine(vmName);
        assertThat(result.payloadStarted).isTrue();
    }
}
