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

import static com.google.common.truth.Truth.assertThat;
import static com.google.common.truth.TruthJUnit.assume;

import android.os.Build;
import android.os.ParcelFileDescriptor;
import android.os.SystemProperties;
import android.system.virtualmachine.VirtualMachine;
import android.util.Log;

import com.android.microdroid.test.device.MicrodroidDeviceTestBase;
import com.android.microdroid.testservice.ITestService;

import org.junit.Before;
import org.junit.Rule;
import org.junit.rules.Timeout;
import org.junit.runners.Parameterized;

import java.io.FileInputStream;
import java.util.concurrent.CompletableFuture;

class MicrodroidTestsBase extends MicrodroidDeviceTestBase {
    protected static final String TAG = "MicrodroidTests";
    private static final String KERNEL_VERSION = SystemProperties.get("ro.kernel.version");
    private static final int MIN_MEM_ARM64 = 150;
    private static final int MIN_MEM_X86_64 = 196;

    @Rule public Timeout globalTimeout = Timeout.seconds(300);

    @Parameterized.Parameter public boolean mProtectedVm;

    @Parameterized.Parameters(name = "protectedVm={0}")
    public static Object[] protectedVmConfigs() {
        return new Object[]{false, true};
    }

    @Before
    public void setup() {
        prepareTestSetup(mProtectedVm);
    }

    protected final void assumeSupportedKernel() {
        assume()
                .withMessage("Skip on 5.4 kernel. b/218303240")
                .that(KERNEL_VERSION)
                .isNotEqualTo("5.4");
    }

    protected final int minMemoryRequired() {
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

    static class TestResults {
        Exception mException;
        Integer mAddInteger;
        String mAppRunProp;
        String mSublibRunProp;
        String mExtraApkTestProp;
    }

    protected final TestResults runVmTestService(VirtualMachine vm) throws Exception {
        CompletableFuture<Boolean> payloadStarted = new CompletableFuture<>();
        CompletableFuture<Boolean> payloadReady = new CompletableFuture<>();
        TestResults testResults = new TestResults();
        VmEventListener listener =
                new VmEventListener() {
                    private void testVMService(VirtualMachine vm) {
                        try {
                            ITestService testService = ITestService.Stub.asInterface(
                                    vm.connectToVsockServer(ITestService.SERVICE_PORT));
                            testResults.mAddInteger = testService.addInteger(123, 456);
                            testResults.mAppRunProp =
                                    testService.readProperty("debug.microdroid.app.run");
                            testResults.mSublibRunProp =
                                    testService.readProperty("debug.microdroid.app.sublib.run");
                            testResults.mExtraApkTestProp =
                                    testService.readProperty("debug.microdroid.test.extra_apk");
                        } catch (Exception e) {
                            testResults.mException = e;
                        }
                    }

                    @Override
                    public void onPayloadReady(VirtualMachine vm) {
                        Log.i(TAG, "onPayloadReady");
                        payloadReady.complete(true);
                        testVMService(vm);
                        forceStop(vm);
                    }

                    @Override
                    public void onPayloadStarted(VirtualMachine vm, ParcelFileDescriptor stream) {
                        Log.i(TAG, "onPayloadStarted");
                        payloadStarted.complete(true);
                        logVmOutput(TAG, new FileInputStream(stream.getFileDescriptor()),
                                "Payload");
                    }
                };
        listener.runToFinish(TAG, vm);
        assertThat(payloadStarted.getNow(false)).isTrue();
        assertThat(payloadReady.getNow(false)).isTrue();
        return testResults;
    }
}
