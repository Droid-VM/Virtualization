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

package android.avf.test;

import static com.android.tradefed.testtype.DeviceJUnit4ClassRunner.TestMetrics;

import static com.google.common.truth.Truth.assertWithMessage;

import static org.junit.Assert.assertTrue;
import static org.junit.Assume.assumeFalse;
import static org.junit.Assume.assumeTrue;

import android.platform.test.annotations.RootPermissionTest;

import com.android.microdroid.test.common.MetricsProcessor;
import com.android.microdroid.test.host.CommandRunner;
import com.android.microdroid.test.host.MicrodroidHostTestCaseBase;
import com.android.tradefed.device.DeviceNotAvailableException;
import com.android.tradefed.log.LogUtil.CLog;
import com.android.tradefed.testtype.DeviceJUnit4ClassRunner;
import com.android.tradefed.util.CommandResult;

import org.junit.After;
import org.junit.Before;
import org.junit.Rule;
import org.junit.Test;
import org.junit.runner.RunWith;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

@RootPermissionTest
@RunWith(DeviceJUnit4ClassRunner.class)
public final class AVFHostTestCase extends MicrodroidHostTestCaseBase {

    private static final String COMPOSD_CMD_BIN = "/apex/com.android.compos/bin/composd_cmd";

    // Files that define the "test" instance of CompOS
    private static final String COMPOS_TEST_ROOT = "/data/misc/apexdata/com.android.compos/test/";

    private static final String SYSTEM_SERVER_COMPILER_FILTER_PROP_NAME =
            "dalvik.vm.systemservercompilerfilter";

    private static final String BOOTLOADER_TIME_PROP_NAME = "ro.boot.boottime";
    private static final String BOOTLOADER_PREFIX = "bootloader-";
    private static final String BOOTLOADER_TIME = "bootloader_time";
    private static final String BOOTLOADER_PHASE_SW = "SW";

    /** Boot time test related variables */
    private static final int REINSTALL_APEX_RETRY_INTERVAL_MS = 5 * 1000;
    private static final int REINSTALL_APEX_TIMEOUT_SEC = 15;
    private static final int COMPILE_STAGED_APEX_RETRY_INTERVAL_MS = 10 * 1000;
    private static final int COMPILE_STAGED_APEX_TIMEOUT_SEC = 540;
    private static final int BOOT_COMPLETE_TIMEOUT_MS = 10 * 60 * 1000;
    private static final double NANOS_IN_SEC = 1_000_000_000.0;
    private static final int ROUND_COUNT = 5;
    private static final String APK_NAME = "MicrodroidTestApp.apk";
    private static final String PACKAGE_NAME = "com.android.microdroid.test";

    // Number of vCPUs for testing purpose
    private static final int NUM_VCPUS = 3;
    private MetricsProcessor mMetricsProcessor;
    @Rule public TestMetrics mMetrics = new TestMetrics();

    @Before
    public void setUp() throws Exception {
        testIfDeviceIsCapable(getDevice());
        mMetricsProcessor = new MetricsProcessor(getMetricPrefix() + "hostside/");
    }

    @After
    public void tearDown() throws Exception {
        // Set PKVM enable and reboot to prevent previous staged session.
        if (!isCuttlefish()) {
            setPKVMStatusWithRebootToBootloader(true);
            rebootFromBootloaderAndWaitBootCompleted();
        }

        CommandRunner android = new CommandRunner(getDevice());

        // Clear up any CompOS instance files we created.
        android.tryRun("rm", "-rf", COMPOS_TEST_ROOT);
    }

    @Test
    public void testBootEnablePKVM() throws Exception {
        enableDisablePKVMTestHelper(true);
    }

    @Test
    public void testBootDisablePKVM() throws Exception {
        enableDisablePKVMTestHelper(false);
    }

    @Test
    public void testBootWithCompOS() throws Exception {
        composTestHelper(true);
    }

    @Test
    public void testBootWithoutCompOS() throws Exception {
        composTestHelper(false);
    }

    @Test
    public void testAppStartupTimeAfterVm() throws Exception {
        float mTotalTimeAvg, mWaitTimeAvg;
        mTotalTimeAvg = mWaitTimeAvg = 0.0f;
        for (int round = 0; round < ROUND_COUNT; ++round) {
            ArrayList<Float> appStartupTime = getAppStartupTimeDelta();
            mTotalTimeAvg += appStartupTime.get(0);
            mWaitTimeAvg += appStartupTime.get(1);
        }

        mTotalTimeAvg /= ROUND_COUNT;
        mWaitTimeAvg /= ROUND_COUNT;

        mMetrics.addTestMetric("settings_app_startup_delta/total_time_ms",
                Float.toString(mTotalTimeAvg));
        mMetrics.addTestMetric("settings_app_startup_delta/wait_time_ms",
                Float.toString(mWaitTimeAvg));
    }

    private void waitForBootComplete() {
        runOnMicrodroidForResult("watch -e \"getprop dev.bootcomplete | grep '^0$'\"");
    }

    // Returns an array of two elements containing the delta between the initial app startup time
    // and the time measured after running the VM.
    private ArrayList<Float> getAppStartupTimeDelta()
            throws Exception {
        final String configPath = "assets/vm_config.json";
        final String cid;
        final int vm_mem_mb;
        final String cmd_consume_mem;
        ArrayList<Float> deltaAppStartup = new ArrayList<Float>(2);
        cmd_consume_mem = "cd /mnt/ramdisk && truncate -s %dM sprayMemory && "
            + "dd if=/dev/zero of=sprayMemory bs=1MB count=%d";

        assumeFalse("Skip on CF", isCuttlefish());

        // Reboot the device to run the test without stage2 fragmentation
        getDevice().reboot();
        assertTrue(getDevice().waitForBootComplete(1800));

        // Run the app before the VM run and collect app startup time statistics
        CommandRunner android = new CommandRunner(getDevice());
        unlockScreen(android);
        String beforeVmStartAppLog = android.run("am", "start -W -S com.android.settings");
        assertTrue(beforeVmStartAppLog != null && !beforeVmStartAppLog.isEmpty());
        AmStartupTime beforeVmStartApp = new AmStartupTime(beforeVmStartAppLog);

        // Donate 80% of the available device memory to the VM
        vm_mem_mb = getFreeMemoryInfoMb(android) * 80 / 100;
        cid = startMicrodroid(
                            getDevice(),
                            getBuild(),
                            APK_NAME,
                            PACKAGE_NAME,
                            configPath,
                            true,
                            vm_mem_mb,
                            Optional.of(NUM_VCPUS));
        adbConnectToMicrodroid(getDevice(), cid);
        waitForBootComplete();

        rootMicrodroid();

        runOnMicrodroid("mkdir -p /mnt/ramdisk && chmod 777 /mnt/ramdisk");
        runOnMicrodroid("mount -t tmpfs -o size=32G tmpfs /mnt/ramdisk");

        // Allocate memory for the VM until it fails and make sure that we touch
        // the allocated memory in the guest to be able to create stage2 fragmentation.
        try {
            runOnMicrodroidForResult(String.format(cmd_consume_mem, vm_mem_mb, vm_mem_mb));
        } catch (Exception ex) {

        } finally {
            shutdownMicrodroid(getDevice(), cid);
        }

        // Make sure we unlock the screen and start after we fragmented stage2 memory
        unlockScreen(android);
        String afterVmStartAppLog = android.run("am", "start -W -S com.android.settings");
        assertTrue(afterVmStartAppLog != null && !afterVmStartAppLog.isEmpty());
        AmStartupTime afterVmStartApp = new AmStartupTime(afterVmStartAppLog);

        deltaAppStartup.set(0, (float) Math.abs(afterVmStartApp.getTotalTime()
                - beforeVmStartApp.getTotalTime()));
        deltaAppStartup.set(1, (float) Math.abs(afterVmStartApp.getWaitTime()
                - beforeVmStartApp.getWaitTime()));

        return deltaAppStartup;
    }

    class AmStartupTime {
        private int mTotalTime;
        private int mWaitTime;

        AmStartupTime(String startAppLog) {
            String[] lines = startAppLog.split("[\r\n]+");
            mTotalTime = mWaitTime = 0;

            for (int i = 0; i < lines.length; i++) {
                if (lines[i].contains("TotalTime:")) {
                    mTotalTime = Integer
                        .parseInt(lines[i].replaceAll("\\D+", ""));
                }
                if (lines[i].contains("WaitTime:")) {
                    mWaitTime = Integer
                        .parseInt(lines[i].replaceAll("\\D+", ""));
                }
            }
        }

        public int getTotalTime() {
            return mTotalTime;
        }

        public int getWaitTime() {
            return mWaitTime;
        }
    }

    private int getFreeMemoryInfoMb(CommandRunner android) throws DeviceNotAvailableException {
        int freeMemory = 0;
        String content = android.runForResult("cat /proc/meminfo").getStdout().trim();
        String[] lines = content.split("[\r\n]+");

        for (int i = 0; i < lines.length; i++) {
            if (lines[i].contains("MemFree:")) {
                freeMemory = Integer.parseInt(lines[i].replaceAll("\\D+", "")) / 1024;
                break;
            }
        }

        return freeMemory;
    }

    private void unlockScreen(CommandRunner android)
            throws DeviceNotAvailableException, InterruptedException {
        android.run("input keyevent", "KEYCODE_WAKEUP");
        Thread.sleep(100);
        final String ret = android.runForResult("dumpsys nfc | grep 'mScreenState='")
                .getStdout().trim();
        if (ret != null && ret.contains("ON_LOCKED")) {
            android.run("input keyevent", "KEYCODE_MENU");
        }
    }

    private void updateBootloaderTimeInfo(Map<String, List<Double>> bootloaderTime)
            throws Exception {

        String bootLoaderVal = getDevice().getProperty(BOOTLOADER_TIME_PROP_NAME);
        // Sample Output : 1BLL:89,1BLE:590,2BLL:0,2BLE:1344,SW:6734,KL:1193
        if (bootLoaderVal != null) {
            String[] bootLoaderPhases = bootLoaderVal.split(",");
            double bootLoaderTotalTime = 0d;
            for (String bootLoaderPhase : bootLoaderPhases) {
                String[] bootKeyVal = bootLoaderPhase.split(":");
                String key = String.format("%s%s", BOOTLOADER_PREFIX, bootKeyVal[0]);

                bootloaderTime.computeIfAbsent(key,
                        k -> new ArrayList<>()).add(Double.parseDouble(bootKeyVal[1]));
                // SW is the time spent on the warning screen. So ignore it in
                // final boot time calculation.
                if (BOOTLOADER_PHASE_SW.equalsIgnoreCase(bootKeyVal[0])) {
                    continue;
                }
                bootLoaderTotalTime += Double.parseDouble(bootKeyVal[1]);
            }
            bootloaderTime.computeIfAbsent(BOOTLOADER_TIME,
                    k -> new ArrayList<>()).add(bootLoaderTotalTime);
        }
    }

    private Double getDmesgBootTime() throws Exception {

        CommandRunner android = new CommandRunner(getDevice());
        String result = android.run("dmesg");
        Pattern pattern = Pattern.compile("\\[(.*)\\].*sys.boot_completed=1.*");
        for (String line : result.split("[\r\n]+")) {
            Matcher matcher = pattern.matcher(line);
            if (matcher.find()) {
                return Double.valueOf(matcher.group(1));
            }
        }
        throw new IllegalArgumentException("Failed to get boot time info.");
    }

    private void enableDisablePKVMTestHelper(boolean isEnable) throws Exception {
        skipIfPKVMStatusSwitchNotSupported();

        List<Double> bootDmesgTime = new ArrayList<>(ROUND_COUNT);
        Map<String, List<Double>> bootloaderTime = new HashMap<>();

        setPKVMStatusWithRebootToBootloader(isEnable);
        rebootFromBootloaderAndWaitBootCompleted();
        for (int round = 0; round < ROUND_COUNT; ++round) {
            getDevice().nonBlockingReboot();
            waitForBootCompleted();

            updateBootloaderTimeInfo(bootloaderTime);

            double elapsedSec = getDmesgBootTime();
            bootDmesgTime.add(elapsedSec);
        }

        String suffix = "";
        if (isEnable) {
            suffix = "enable";
        } else {
            suffix = "disable";
        }

        reportMetric(bootDmesgTime, "dmesg_boot_time_with_pkvm_" + suffix, "s");
        reportAggregatedMetrics(bootloaderTime,
                "bootloader_time_with_pkvm_" + suffix, "ms");
    }

    private void composTestHelper(boolean isWithCompos) throws Exception {
        assumeFalse("Skip on CF; too slow", isCuttlefish());

        List<Double> bootDmesgTime = new ArrayList<>(ROUND_COUNT);

        for (int round = 0; round < ROUND_COUNT; ++round) {
            reInstallApex(REINSTALL_APEX_TIMEOUT_SEC);
            if (isWithCompos) {
                compileStagedApex(COMPILE_STAGED_APEX_TIMEOUT_SEC);
            }
            getDevice().nonBlockingReboot();
            waitForBootCompleted();

            double elapsedSec = getDmesgBootTime();
            bootDmesgTime.add(elapsedSec);
        }

        String suffix = "";
        if (isWithCompos) {
            suffix = "with_compos";
        } else {
            suffix = "without_compos";
        }

        reportMetric(bootDmesgTime, "dmesg_boot_time_" + suffix, "s");
    }

    private void skipIfPKVMStatusSwitchNotSupported() throws Exception {
        assumeFalse("Skip on CF; can't reboot to bootloader", isCuttlefish());

        if (!getDevice().isStateBootloaderOrFastbootd()) {
            getDevice().rebootIntoBootloader();
        }
        getDevice().waitForDeviceBootloader();

        CommandResult result;
        result = getDevice().executeFastbootCommand("oem", "pkvm", "status");
        rebootFromBootloaderAndWaitBootCompleted();
        assumeFalse(result.getStderr().contains("Invalid oem command"));
        // Skip the test if running on a build with pkvm_enabler. Disabling pKVM
        // for such builds results in a bootloop.
        assumeTrue(result.getStderr().contains("misc=auto"));
    }

    private void reportMetric(List<Double> data, String name, String unit) {
        CLog.d("Report metric " + name + "(" + unit + ") : " + data.toString());
        Map<String, Double> stats = mMetricsProcessor.computeStats(data, name, unit);
        for (Map.Entry<String, Double> entry : stats.entrySet()) {
            CLog.d("Add test metrics " + entry.getKey() + " : " + entry.getValue().toString());
            mMetrics.addTestMetric(entry.getKey(), entry.getValue().toString());
        }
    }

    private void reportAggregatedMetrics(Map<String, List<Double>> bootloaderTime,
            String prefix, String unit) {

        for (Map.Entry<String, List<Double>> entry : bootloaderTime.entrySet()) {
            reportMetric(entry.getValue(), prefix + "_" + entry.getKey(), unit);
        }
    }

    private void setPKVMStatusWithRebootToBootloader(boolean isEnable) throws Exception {

        if (!getDevice().isStateBootloaderOrFastbootd()) {
            getDevice().rebootIntoBootloader();
        }
        getDevice().waitForDeviceBootloader();

        CommandResult result;
        if (isEnable) {
            result = getDevice().executeFastbootCommand("oem", "pkvm", "enable");
        } else {
            result = getDevice().executeFastbootCommand("oem", "pkvm", "disable");
        }

        result = getDevice().executeFastbootCommand("oem", "pkvm", "status");
        CLog.i("Gets PKVM status : " + result);

        String expectedOutput = "";

        if (isEnable) {
            expectedOutput = "pkvm is enabled";
        } else {
            expectedOutput = "pkvm is disabled";
        }
        assertWithMessage("Failed to set PKVM status. Reason: " + result)
            .that(result.toString()).ignoringCase().contains(expectedOutput);
    }

    private void rebootFromBootloaderAndWaitBootCompleted() throws Exception {
        getDevice().executeFastbootCommand("reboot");
        getDevice().waitForDeviceOnline(BOOT_COMPLETE_TIMEOUT_MS);
        getDevice().waitForBootComplete(BOOT_COMPLETE_TIMEOUT_MS);
        getDevice().enableAdbRoot();
    }

    private void waitForBootCompleted() throws Exception {
        getDevice().waitForDeviceOnline(BOOT_COMPLETE_TIMEOUT_MS);
        getDevice().waitForBootComplete(BOOT_COMPLETE_TIMEOUT_MS);
        getDevice().enableAdbRoot();
    }

    private void compileStagedApex(int timeoutSec) throws Exception {

        long timeStart = System.currentTimeMillis();
        long timeEnd = timeStart + timeoutSec * 1000;

        while (true) {

            try {
                CommandRunner android = new CommandRunner(getDevice());

                String result = android.run(
                        COMPOSD_CMD_BIN + " staged-apex-compile");
                assertWithMessage("Failed to compile staged APEX. Reason: " + result)
                    .that(result).ignoringCase().contains("all ok");

                CLog.i("Success to compile staged APEX. Result: " + result);

                break;
            } catch (AssertionError e) {
                CLog.i("Gets AssertionError when compile staged APEX. Detail: " + e);
            }

            if (System.currentTimeMillis() > timeEnd) {
                CLog.e("Try to compile staged APEX several times but all fail.");
                throw new AssertionError("Failed to compile staged APEX.");
            }

            Thread.sleep(COMPILE_STAGED_APEX_RETRY_INTERVAL_MS);
        }
    }

    private void reInstallApex(int timeoutSec) throws Exception {

        long timeStart = System.currentTimeMillis();
        long timeEnd = timeStart + timeoutSec * 1000;

        while (true) {

            try {
                CommandRunner android = new CommandRunner(getDevice());

                String packagesOutput =
                        android.run("pm list packages -f --apex-only");

                Pattern p = Pattern.compile(
                        "package:(.*)=(com(?:\\.google)?\\.android\\.art)$", Pattern.MULTILINE);
                Matcher m = p.matcher(packagesOutput);
                assertWithMessage("ART module not found. Packages are:\n" + packagesOutput)
                    .that(m.find())
                    .isTrue();

                String artApexPath = m.group(1);

                CommandResult result = android.runForResult(
                        "pm install --apex " + artApexPath);
                assertWithMessage("Failed to install APEX. Reason: " + result)
                    .that(result.getExitCode()).isEqualTo(0);

                CLog.i("Success to install APEX. Result: " + result);

                break;
            } catch (AssertionError e) {
                CLog.i("Gets AssertionError when reinstall art APEX. Detail: " + e);
            }

            if (System.currentTimeMillis() > timeEnd) {
                CLog.e("Try to reinstall art APEX several times but all fail.");
                throw new AssertionError("Failed to reinstall art APEX.");
            }

            Thread.sleep(REINSTALL_APEX_RETRY_INTERVAL_MS);
        }
    }
}
