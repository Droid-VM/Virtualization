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
import static com.google.common.truth.Truth.assertWithMessage;
import static com.google.common.truth.TruthJUnit.assume;

import android.system.virtualmachine.VirtualMachine;
import android.system.virtualmachine.VirtualMachineConfig;

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

/**
 * Test class which contains long-running stress tests of the AVF API. These push resource usage
 * beyond the expected usage patterns of a typical client, and assert that the device either
 * succeeds or handles the failure gracefully.
 */
@RunWith(Parameterized.class)
public class MicrodroidStressTests extends MicrodroidDeviceTestBase {
    private static final String TAG = "MicrodroidStressTests";
    private static final long ONE_MEBI = 1024 * 1024;
    private static final long MEM_SIZE = 128 * ONE_MEBI;

    @Rule public Timeout globalTimeout = Timeout.seconds(3600);

    @Parameterized.Parameters(name = "protectedVm={0}")
    public static Object[] protectedVmConfigs() {
        return new Object[] {false, true};
    }

    @Parameterized.Parameter public boolean mProtectedVm;

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
        prepareTestSetup(mProtectedVm);
    }

    @Test
    public void testBootVms_Serial() throws Exception {
        assume().withMessage("Skip on CF; too slow").that(isCuttlefish()).isFalse();

        int trialCount = 300;

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
            assertThat(result.payloadStarted).isTrue();
        }
    }

    @Test
    public void testStartVms_Parallel() throws Exception {
        // Running a lot of VMs should not kill the device. We might, however,
        // start hitting VM response timeouts, so don't expect them all to
        // boot successfully.
        int cpus = Runtime.getRuntime().availableProcessors();
        assertCanStartVMsInParallel(cpus * 8, /* assertPayloadStarted */ false);
    }

    @Test
    public void testBootVms_Parallel() throws Exception {
        // One single-vCPU VM per core should be able to boot in parallel and
        // within the time limit.
        int cpus = Runtime.getRuntime().availableProcessors();
        assertCanStartVMsInParallel(cpus, /* assertPayloadStarted */ true);
    }

    private void assertCanStartVMsInParallel(int threadCount, boolean assertPayloadStarted)
            throws Exception {
        assume().withMessage("Skip on CF; too slow").that(isCuttlefish()).isFalse();

        // Signals to the main thread that a secondary thread is ready.
        Semaphore semReady = new Semaphore(threadCount);
        semReady.drainPermits();

        // Signals to the secondary thread to start booting its VM.
        Semaphore semStart = new Semaphore(threadCount);
        semStart.drainPermits();

        Map<Thread, ThreadInfo> threads = new HashMap<>();
        for (int i = 0; i < threadCount; i++) {
            String vmName = "stress_test_vm_" + i;
            ThreadInfo info = new ThreadInfo(i);

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

                                    // Signal to the main thread that we are ready to boot the VM.
                                    // Wait for the main thread to release permits to boot the VMs.
                                    semReady.release();
                                    semStart.acquireUninterruptibly();

                                    BootResult result = tryBootVm(TAG, vmName);
                                    info.setSuccessful(result.payloadStarted);
                                } catch (Exception ex) {
                                    info.setException(ex);
                                }
                            });
            thread.start();
            threads.put(thread, info);
        }

        // Wait for all threads to confirm they are ready, then release permits to boot the VMs.
        semReady.acquireUninterruptibly(threadCount);
        semStart.release(threadCount);

        // Wait for all threads to finish.
        for (Map.Entry<Thread, ThreadInfo> entry : threads.entrySet()) {
            entry.getKey().join();

            ThreadInfo info = entry.getValue();
            assertWithMessage("Thread " + info.getId() + " threw an exception")
                    .that(info.getException())
                    .isNull();
            if (assertPayloadStarted) {
                assertWithMessage("Thread " + info.getId() + " did not boot successfully")
                        .that(info.isSuccessful())
                        .isTrue();
            }
        }
    }
}
