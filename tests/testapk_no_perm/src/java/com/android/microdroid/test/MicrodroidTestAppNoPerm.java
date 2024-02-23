/*
 * Copyright 2024 The Android Open Source Project
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

import android.os.Build;
import android.system.virtualmachine.VirtualMachineConfig;

import com.android.compatibility.common.util.CddTest;
import com.android.microdroid.test.device.MicrodroidDeviceTestBase;

import static com.google.common.truth.Truth.assertThat;
import static org.junit.Assert.assertThrows;

import org.junit.Test;
import org.junit.runner.RunWith;
import org.junit.runners.JUnit4;

/**
 * Test that the android.permission.MANAGE_VIRTUAL_MACHINE is enforced and that an app cannot launch
 * a VM without said permission.
 */
@RunWith(JUnit4.class)
public class MicrodroidTestAppNoPerm extends MicrodroidDeviceTestBase {
    private static final long ONE_MEBI = 1024 * 1024;
    private static final long MIN_MEM_ARM64 = 170 * ONE_MEBI;
    private static final long MIN_MEM_X86_64 = 196 * ONE_MEBI;

    private long minMemoryRequired() {
        if (Build.SUPPORTED_ABIS.length > 0) {
            String primaryAbi = Build.SUPPORTED_ABIS[0];
            switch (primaryAbi) {
                case "x86_64":
                    return MIN_MEM_X86_64;
                case "arm64-v8a":
                    return MIN_MEM_ARM64;
            }
        }
        return 0;
    }

    @Test
    @CddTest(
            requirements = {
                "9.17/C-1-1",
                "9.17/C-1-2",
                "9.17/C-1-4",
            })
    public void createVmRequiresPermission() {
        assumeSupportedDevice();

        VirtualMachineConfig config =
                newVmConfigBuilderWithPayloadBinary("MicrodroidTestNativeLib.so")
                        .setMemoryBytes(minMemoryRequired())
                        .build();

        SecurityException e =
                assertThrows(
                        SecurityException.class,
                        () -> forceCreateNewVirtualMachine("test_vm_requires_permission", config));
        assertThat(e)
                .hasMessageThat()
                .contains("android.permission.MANAGE_VIRTUAL_MACHINE permission");
    }
}
