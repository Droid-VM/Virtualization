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

package android.avf.test;

import com.android.tradefed.device.ITestDevice;
import com.android.tradefed.device.TestDevice.MicrodroidBuilder;
import com.android.tradefed.invoker.TestInformation;
import com.android.tradefed.testtype.DeviceJUnit4ClassRunner.TestMetrics;
import com.android.tradefed.testtype.junit4.BaseHostJUnit4Test;
import com.android.tradefed.testtype.junit4.BeforeClassWithInfo;

import static com.google.common.truth.Truth.assertWithMessage;
import static org.junit.Assert.assertNotNull;

import android.platform.test.annotations.RootPermissionTest;

import com.android.tradefed.device.DeviceNotAvailableException;
import com.android.tradefed.device.TestDevice;
import com.android.tradefed.log.LogUtil.CLog;
import com.android.tradefed.testtype.DeviceJUnit4ClassRunner;
import com.android.tradefed.util.FileUtil;

import org.junit.After;
import org.junit.Before;
import org.junit.Rule;
import org.junit.Test;
import org.junit.runner.RunWith;

import java.io.File;

@RootPermissionTest
@RunWith(DeviceJUnit4ClassRunner.class)
public class MicrodroidDVFSHostTestCase extends BaseHostJUnit4Test {

    private final int mIterations = 20;
    private static final String PACKAGE_NAME = "com.android.microdroid.test";
    private static final int BOOT_COMPLETE_TIMEOUT_MS = 10 * 60 * 1000;

    @Rule public TestMetrics mMetrics = new TestMetrics();

    private CpuDvfsTestHelper mHelper;
    private ITestDevice mMicrodroidDevice;
    TestDevice mHostDevice;

    @BeforeClassWithInfo
    public static void rebootDevice(TestInformation testInfo) throws Exception {
        assertNotNull(testInfo.getDevice());
        testInfo.getDevice().executeShellV2Command("reboot");
        testInfo.getDevice().waitForDeviceOnline(BOOT_COMPLETE_TIMEOUT_MS);
        testInfo.getDevice().waitForBootComplete(BOOT_COMPLETE_TIMEOUT_MS);
        testInfo.getDevice().enableAdbRoot();
    }

    @Before
    public void setUp() throws Exception {
        mHostDevice = (TestDevice) getDevice();

        // Donate 80% of the available mHostDevice memory to the VM
        final String configPath = "assets/microdroid/vm_config.json";
        final int vm_mem_mb = getFreeMemoryInfoMb(mHostDevice) * 80 / 100;

        mMicrodroidDevice =
                MicrodroidBuilder.fromDevicePath(
                                getPathForPackage(getDevice(), PACKAGE_NAME), configPath)
                        .debugLevel("full")
                        .memoryMib(vm_mem_mb)
                        .cpuTopology("match_host")
                        .build(mHostDevice);
        mMicrodroidDevice.waitForBootComplete(30000);
        mMicrodroidDevice.enableAdbRoot();

        mHelper = new CpuDvfsTestHelper(mMicrodroidDevice);

        File tempDir;
        tempDir = FileUtil.createTempDir("dvfs_tools");
        if (!mHostDevice.pullDir(mHelper.BASE_DIR, tempDir)) {
            CLog.w(
                    "Failed to pull directory %s from mHostDevice %s",
                    mHelper.BASE_DIR, mHostDevice.getSerialNumber());
        }

        mMicrodroidDevice.executeShellV2Command(
                "umount /data && mount -w -t tmpfs /data /data && mkdir /data/local && mkdir"
                        + " /data/local/tmp");
        mMicrodroidDevice.pushDir(tempDir, mHelper.BASE_DIR);

        mHelper.prepMicrodroid();
    }

    @After
    public void tearDown() throws Exception {
        mHostDevice.shutdownMicrodroid(mMicrodroidDevice);
    }

    @Test
    public void bigUpMigrateFMaxTest() throws Exception {
        double latency;
        latency = mHelper.runRTApp("upmigrate_big.json", "upmigrate_big.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSbigUpmigrateFMaxMs", String.format("%.3f", latency));
    }

    /**
     * littleFMinToFMaxTest: Measures the latency for little CPU to reach Fmax from Fmin given a
     * thread that initially runs with a low load and suddenly transitions to a high load.
     */
    @Test
    public void littleFMinToFMaxTest() throws Exception {
        double latency;
        latency = mHelper.runRTApp("rampup_little.json", "rampup_little.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSlittleFMinToFMaxMs", String.format("%.3f", latency));
    }

    /**
     * middleFMinToFMaxTest: Measures the latency for middle CPU to reach Fmax from Fmin given a
     * thread that initially runs with a low load and suddenly transitions to a high load.
     */
    @Test
    public void middleFMinToFMaxTest() throws Exception {
        double latency;

        latency = mHelper.runRTApp("rampup_middle.json", "rampup_middle.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSmiddleFMinToFMaxMs", String.format("%.3f", latency));
    }

    /**
     * bigFMinToFMaxTest: Measures the latency for big CPU to reach Fmax from Fmin given a thread
     * that initially runs with a low load and suddenly transitions to a high load.
     */
    @Test
    public void bigFMinToFMaxTest() throws Exception {
        double latency;

        latency = mHelper.runRTApp("rampup_big.json", "rampup_big.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSbigFMinToFMaxMs", String.format("%.3f", latency));
    }

    /**
     * littleNewFMinToFMaxTest: Measures the latency for little CPU to reach Fmax from Fmin given a
     * newly spawned heavy workload.
     */
    @Test
    public void littleNewFMinToFMaxTest() throws Exception {
        double latency;
        latency =
                mHelper.runRTApp("new_rampup_little.json", "new_rampup_little.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSlittleNewFMinToFMaxMs", String.format("%.3f", latency));
    }

    /**
     * middleNewFMinToFMaxTest: Measures the latency for middle CPU to reach Fmax from Fmin given a
     * newly spawned heavy workload.
     */
    @Test
    public void middleNewFMinToFMaxTest() throws Exception {
        double latency;
        latency =
                mHelper.runRTApp("new_rampup_middle.json", "new_rampup_middle.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSmiddleNewFMinToFMaxMs", String.format("%.3f", latency));
    }

    /**
     * bigNewFMinToFMaxTest: Measures the latency for big CPU to reach Fmax from Fmin given a newly
     * spawned heavy workload.
     */
    @Test
    public void bigNewFMinToFMaxTest() throws Exception {
        double latency;
        latency = mHelper.runRTApp("new_rampup_big.json", "new_rampup_big.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSbigNewFMinToFMaxMs", String.format("%.3f", latency));
    }

    /**
     * little50PercentLoadTest: Measures the latency for little CPU to reach the steady state
     * frequency corresponding to a 50% load from FMin
     */
    @Test
    public void little50PercentLoadTest() throws Exception {
        double latency;
        latency =
                mHelper.runRTApp(
                        "rampup_little_50load.json", "rampup_little_50load.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSlittle50PercentLoadMs", String.format("%.3f", latency));
    }

    /**
     * middle50PercentLoadTest: Measures the latency for middle CPU to reach the steady state
     * frequency corresponding to a 50% load from FMin
     */
    @Test
    public void middle50PercentLoadTest() throws Exception {
        double latency;
        latency =
                mHelper.runRTApp(
                        "rampup_middle_50load.json", "rampup_middle_50load.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSmiddle50PercentLoadMs", String.format("%.3f", latency));
    }

    /**
     * big50PercentLoadTest: Measures the latency for big CPU to reach the steady state frequency
     * corresponding to a 50% load from FMin
     */
    @Test
    public void big50PercentLoadTest() throws Exception {
        double latency;
        latency =
                mHelper.runRTApp("rampup_big_50load.json", "rampup_big_50load.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSbig50PercentLoadMs", String.format("%.3f", latency));
    }

    /**
     * littleRampdownTest: Measures the latency for a thread with initially large workload to ramp
     * down to FMin given a 5% workload on little CPU.
     */
    @Test
    public void littleRampdownTest() throws Exception {
        double latency;
        latency = mHelper.runRTApp("rampdown_little.json", "rampdown_little.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSlittleRampdownMs", String.format("%.3f", latency));
    }

    /**
     * middleRampdownTest: Measures the latency for a thread with initially large workload to ramp
     * down to FMin given a 5% workload on middle CPU.
     */
    @Test
    public void middleRampdownTest() throws Exception {
        double latency;
        latency = mHelper.runRTApp("rampdown_middle.json", "rampdown_middle.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSmiddleRampdownMs", String.format("%.3f", latency));
    }

    /**
     * bigRampdownTest: Measures the latency for a thread with initially large workload to ramp down
     * to FMin given a 5% workload on big CPU.
     */
    @Test
    public void bigRampdownTest() throws Exception {
        double latency;
        latency = mHelper.runRTApp("rampdown_big.json", "rampdown_big.query", mIterations);
        mMetrics.addTestMetric("CPUVMDVFSbigRampdownMs", String.format("%.3f", latency));
    }

    private int getFreeMemoryInfoMb(ITestDevice device)
            throws DeviceNotAvailableException, IllegalArgumentException {
        int freeMemory = 0;
        String content = device.executeShellV2Command("cat /proc/meminfo").getStdout().trim();
        String[] lines = content.split("[\r\n]+");

        for (String line : lines) {
            if (line.contains("MemFree:")) {
                freeMemory = Integer.parseInt(line.replaceAll("\\D+", "")) / 1024;
                return freeMemory;
            }
        }

        throw new IllegalArgumentException();
    }

    private String getPathForPackage(ITestDevice device, String packageName)
            throws DeviceNotAvailableException {
        String pathLine = device.executeShellV2Command("pm path " + packageName).getStdout().trim();
        assertWithMessage("Package " + packageName + " not found")
                .that(pathLine)
                .startsWith("package:");
        return pathLine.substring("package:".length());
    }
}
