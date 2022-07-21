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

package com.android.virt.fs;

import static android.virt.test.LogArchiver.archiveLogThenDelete;

import static com.android.tradefed.device.TestDevice.MicrodroidBuilder;
import static com.android.tradefed.testtype.DeviceJUnit4ClassRunner.TestLogData;

import static com.google.common.truth.Truth.assertThat;

import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.fail;
import static org.junit.Assume.assumeTrue;

import android.virt.test.CommandRunner;

import com.android.compatibility.common.tradefed.build.CompatibilityBuildHelper;
import com.android.tradefed.build.IBuildInfo;
import com.android.tradefed.device.DeviceNotAvailableException;
import com.android.tradefed.device.ITestDevice;
import com.android.tradefed.device.TestDevice;
import com.android.tradefed.invoker.TestInformation;
import com.android.tradefed.log.LogUtil.CLog;

import org.junit.runner.Description;
import org.junit.runners.model.Statement;

import java.io.File;
import java.io.FileNotFoundException;

/** Custom TestRule for AuthFs tests. */
public class AuthFsTestRule extends TestLogData {
    /** VM config entry path in the test APK */
    private static final String VM_CONFIG_PATH_IN_APK = "assets/vm_config.json";

    /** Test directory on Android where data are located */
    private static final String TEST_DIR = "/data/local/tmp/authfs";

    /** File name of the test APK */
    private static final String TEST_APK_NAME = "MicrodroidTestApp.apk";

    /** Output directory where the test can generate output on Android */
    private static final String TEST_OUTPUT_DIR = "/data/local/tmp/authfs/output_dir";

    /** Mount point of authfs on Microdroid during the test */
    private static final String MOUNT_DIR = "/data/local/tmp";

    /** VM's log file */
    private static final String LOG_PATH = TEST_OUTPUT_DIR + "/log.txt";

    /** Path to open_then_run on Android */
    private static final String OPEN_THEN_RUN_BIN = "/data/local/tmp/open_then_run";

    /** Path to fd_server on Android */
    private static final String FD_SERVER_BIN = "/apex/com.android.virt/bin/fd_server";

    /** Path to authfs on Microdroid */
    private static final String AUTHFS_BIN = "/system/bin/authfs";

    private static TestInformation sTestInfo;
    private static CommandRunner sAndroid;
    private static CommandRunner sMicrodroid;

    static void setUpClass(TestInformation testInfo) throws Exception {
        assertNotNull(testInfo.getDevice());
        if (!(testInfo.getDevice() instanceof TestDevice)) {
            CLog.w("Unexpected type of ITestDevice. Skipping.");
            return;
        }
        sTestInfo = testInfo;
        TestDevice androidDevice = getDevice();
        sAndroid = new CommandRunner(androidDevice);

        // NB: We can't use assumeTrue because the assumption exception is NOT handled by the test
        // infra when it is thrown from a class method (see b/37502066). We need to skip both here
        // and in setUp.
        if (!androidDevice.supportsMicrodroid()) {
            CLog.i("Microdroid not supported. Skipping.");
            return;
        }

        // For each test case, boot and adb connect to a new Microdroid
        CLog.i("Starting the shared VM");
        ITestDevice microdroidDevice =
                MicrodroidBuilder.fromFile(
                                findTestApk(testInfo.getBuildInfo()), VM_CONFIG_PATH_IN_APK)
                        .debugLevel("full")
                        .build((TestDevice) androidDevice);

        // From this point on, we need to tear down the Microdroid instance
        sMicrodroid = new CommandRunner(microdroidDevice);

        // Root because authfs (started from shell in this test) currently require root to open
        // /dev/fuse and mount the FUSE.
        assertThat(microdroidDevice.enableAdbRoot()).isTrue();
    }

    static void tearDownClass(TestInformation testInfo) throws DeviceNotAvailableException {
        assertNotNull(sAndroid);

        if (sMicrodroid != null) {
            CLog.i("Shutting down shared VM");
            ((TestDevice) testInfo.getDevice()).shutdownMicrodroid(sMicrodroid.getDevice());
            sMicrodroid = null;
        }

        sAndroid = null;
    }

    static CommandRunner getAndroid() {
        return sAndroid;
    }

    static CommandRunner getMicrodroid() {
        return sMicrodroid;
    }

    @Override
    public Statement apply(final Statement base, Description description) {
        return super.apply(
                new Statement() {
                    @Override
                    public void evaluate() throws Throwable {
                        setUpTest();
                        base.evaluate();
                        tearDownTest(description.getMethodName());
                    }
                },
                description);
    }

    private static File findTestApk(IBuildInfo buildInfo) {
        try {
            return (new CompatibilityBuildHelper(buildInfo)).getTestFile(TEST_APK_NAME);
        } catch (FileNotFoundException e) {
            fail("Missing test file: " + TEST_APK_NAME);
            return null;
        }
    }

    private static TestDevice getDevice() {
        return (TestDevice) sTestInfo.getDevice();
    }

    private void setUpTest() throws Exception {
        assumeTrue(getDevice().supportsMicrodroid());
        sAndroid.run("mkdir -p " + TEST_OUTPUT_DIR);
    }

    private void tearDownTest(String testName) throws Exception {
        if (sMicrodroid != null) {
            sMicrodroid.tryRun("killall authfs");
            sMicrodroid.tryRun("umount " + MOUNT_DIR);
        }

        assertNotNull(sAndroid);
        sAndroid.tryRun("killall fd_server");

        // Even though we only run one VM for the whole class, and could have collect the VM log
        // after all tests are done, TestLogData doesn't seem to work at class level. Hence,
        // collect recent logs manually for each test method.
        String vmRecentLog = TEST_OUTPUT_DIR + "/vm_recent.log";
        sAndroid.tryRun("tail -n 50 " + LOG_PATH + " > " + vmRecentLog);
        archiveLogThenDelete(this, getDevice(), vmRecentLog, "vm_recent.log-" + testName);

        sAndroid.run("rm -rf " + TEST_OUTPUT_DIR);
    }
}
