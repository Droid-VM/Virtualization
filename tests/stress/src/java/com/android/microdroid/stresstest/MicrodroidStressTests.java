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

import static androidx.test.platform.app.InstrumentationRegistry.getInstrumentation;

import static com.google.common.truth.Truth.assertThat;
import static com.google.common.truth.TruthJUnit.assume;

import android.app.Instrumentation;
import android.system.virtualmachine.VirtualMachine;
import android.system.virtualmachine.VirtualMachineConfig;
import android.system.virtualmachine.VirtualMachineException;
import android.util.Log;

import com.android.microdroid.test.device.MicrodroidDeviceTestBase;

import org.junit.Before;
import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.Timeout;
import org.junit.runner.RunWith;
import org.junit.runners.Parameterized;

import java.io.IOException;
import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.Semaphore;

@RunWith(Parameterized.class)
public class MicrodroidStressTests extends MicrodroidDeviceTestBase {
    private static final String TAG = "MicrodroidStressTests";
    private static final long ONE_MEBI = 1024 * 1024;
    private static final long MEM_SIZE = 80 * ONE_MEBI;

    @Rule public Timeout globalTimeout = Timeout.seconds(900);

    @Parameterized.Parameters(name = "protectedVm={0}")
    public static Object[] protectedVmConfigs() {
        return new Object[] {false, true};
    }

    @Parameterized.Parameter public boolean mProtectedVm;

    private Instrumentation mInstrumentation;

    private static class ThreadInfo {
        final int mId;
        Exception mException;
        boolean mSuccessful;

        ThreadInfo(int id) {
            mId = id;
            mException = null;
            mSuccessful = false;
        }

        int getId() {
            return mId;
        }

        synchronized void setException(Exception ex) {
            mException = ex;
        }

        synchronized Exception getException() {
            return mException;
        }

        synchronized void setSuccessful(boolean val) {
            mSuccessful = val;
        }

        synchronized boolean isSuccessful() {
            return mSuccessful;
        }
    }

    @Before
    public void setup() throws IOException {
        grantPermission(VirtualMachine.MANAGE_VIRTUAL_MACHINE_PERMISSION);
        grantPermission(VirtualMachine.USE_CUSTOM_VIRTUAL_MACHINE_PERMISSION);
        prepareTestSetup(mProtectedVm);
        setMaxPerformanceTaskProfile();
        mInstrumentation = getInstrumentation();
    }

    @Test
    public void testStartManyVMsOneByOne() throws Exception {
        assume().withMessage("Skip on CF; too slow").that(isCuttlefish()).isFalse();

        final int trialCount = 300;

        for (int i = 0; i < trialCount; i++) {
            String vmName = "stress_test_vm_" + i;

            forceCreateNewVirtualMachine(
                    vmName,
                    newVmConfigBuilder()
                            .setPayloadBinaryName("MicrodroidIdleNativeLib.so")
                            .setMemoryBytes(MEM_SIZE)
                            .setDebugLevel(DEBUG_LEVEL_NONE)
                            .build());
            BootResult result = tryBootVm(TAG, vmName);
            forceDropVirtualMachine(vmName);
            System.gc();
            System.gc();

            assertThat(result.payloadStarted).isTrue();
        }
    }

    private void assertCanStartVMsInParallel(final int threadCount, boolean assertPayloadStarted)
            throws Exception {
        assume().withMessage("Skip on CF; too slow").that(isCuttlefish()).isFalse();

        // Signals to the main thread that a secondary thread is ready.
        final Semaphore semReady = new Semaphore(threadCount);
        semReady.acquireUninterruptibly(threadCount);

        // Signals to the secondary thread to start booting its VM.
        final Semaphore semStart = new Semaphore(threadCount);
        semStart.acquireUninterruptibly(threadCount);

        Map<Thread, ThreadInfo> threads = new HashMap<>();
        for (int i = 0; i < threadCount; i++) {
            final String vmName = "stress_test_vm_" + i;
            final ThreadInfo info = new ThreadInfo(i);

            Thread thread =
                    new Thread(
                            () -> {
                                VirtualMachineConfig config =
                                        newVmConfigBuilder()
                                                .setPayloadBinaryName("MicrodroidIdleNativeLib.so")
                                                .setMemoryBytes(MEM_SIZE)
                                                .setDebugLevel(DEBUG_LEVEL_NONE)
                                                .build();

                                try {
                                    forceCreateNewVirtualMachine(vmName, config);
                                } catch (VirtualMachineException ex) {
                                    info.setException(ex);
                                    return;
                                }

                                // Signal to the main thread that we are ready to boot the VM.
                                semReady.release();

                                // Wait for the main thread to release permits to boot the VMs.
                                semStart.acquireUninterruptibly();

                                BootResult result;
                                try {
                                    result = tryBootVm(TAG, vmName);
                                } catch (VirtualMachineException | InterruptedException ex) {
                                    info.setException(ex);
                                    return;
                                }

                                info.setSuccessful(result.payloadStarted);
                            });
            thread.start();
            threads.put(thread, info);
        }

        // Wait for all threads to confirm they are ready.
        Log.w(TAG, "XXX main: on your marks...");
        semReady.acquireUninterruptibly(threadCount);

        // Release permits to boot the VMs.
        Log.w(TAG, "XXX main: START!!!");
        semStart.release(threadCount);

        // Wait for all threads to finish.
        for (Map.Entry<Thread, ThreadInfo> entry : threads.entrySet()) {
            Exception ex;
            Thread thread = entry.getKey();
            ThreadInfo info = entry.getValue();
            int id = info.getId();

            thread.join();
            if ((ex = info.getException()) != null) {
                throw new Exception("Thread " + id + " threw an exception", ex);
            }
            if (assertPayloadStarted) {
                assertThat(info.isSuccessful()).isTrue();
            }
        }
    }

    @Test
    public void testStartManyVMsInParallel() throws Exception {
        // Running 128 VMs should not kill the device but we don't expect
        // all of them to boot successfully.
        assertCanStartVMsInParallel(64, /* assertPayloadStarted */ false);
    }

    @Test
    public void testManyVMsInParallelCanBoot() throws Exception {
        // 32 VMs should be able to successfully boot in parallel.
        assertCanStartVMsInParallel(16, /* assertPayloadStarted */ true);
    }
}
