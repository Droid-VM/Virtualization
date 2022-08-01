/*
 * Copyright (C) 2021 The Android Open Source Project
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

package com.android.microdroid.benchmark;

import static com.android.microdroid.test.CommandResultSubject.command_results;
import static com.android.tradefed.testtype.DeviceJUnit4ClassRunner.TestLogData;

import static com.google.common.truth.Truth.assertThat;
import static com.google.common.truth.Truth.assertWithMessage;

import static org.hamcrest.CoreMatchers.containsString;
import static org.hamcrest.CoreMatchers.is;
import static org.hamcrest.CoreMatchers.nullValue;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertThat;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;
import static org.junit.Assume.assumeTrue;

import com.android.microdroid.test.CommandRunner;
import com.android.microdroid.test.MicrodroidHostTestCaseBase;

import com.android.tradefed.device.DeviceNotAvailableException;
import com.android.tradefed.result.TestDescription;
import com.android.tradefed.result.TestResult;
import com.android.tradefed.testtype.DeviceJUnit4ClassRunner;
import com.android.tradefed.testtype.DeviceJUnit4ClassRunner.TestMetrics;
import com.android.tradefed.testtype.junit4.DeviceTestRunOptions;
import com.android.tradefed.util.CommandResult;
import com.android.tradefed.util.FileUtil;
import com.android.tradefed.util.RunUtil;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;
import org.junit.After;
import org.junit.Before;
import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.TestName;
import org.junit.runner.RunWith;

import java.io.File;
import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.concurrent.Callable;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

@RunWith(DeviceJUnit4ClassRunner.class)
public class MicrodroidHostBenchmarks extends MicrodroidHostTestCaseBase {
    private static final String APK_NAME = "MicrodroidBenchmarkApp.apk";
    private static final String PACKAGE_NAME = "com.android.microdroid.benchmark";
    private static final String SHELL_PACKAGE_NAME = "com.android.shell";

    private static final int MIN_MEM_ARM64 = 145;
    private static final int MIN_MEM_X86_64 = 196;

    // Number of vCPUs and their affinity to host CPUs for testing purpose
    private static final int NUM_VCPUS = 3;
    private static final String CPU_AFFINITY = "0,1,2";

    @Rule public TestLogData mTestLogs = new TestLogData();
    @Rule public TestName mTestName = new TestName();
    @Rule public TestMetrics mMetrics = new TestMetrics();

    private int minMemorySize() throws DeviceNotAvailableException {
        CommandRunner android = new CommandRunner(getDevice());
        String abi = android.run("getprop", "ro.product.cpu.abi");
        assertTrue(abi != null && !abi.isEmpty());
        if (abi.startsWith("arm64")) {
            return MIN_MEM_ARM64;
        } else if (abi.startsWith("x86_64")) {
            return MIN_MEM_X86_64;
        }
        fail("Unsupported ABI: " + abi);
        return 0;
    }

    private void waitForBootComplete() {
        runOnMicrodroidForResult("watch -e \"getprop dev.bootcomplete | grep '^0$'\"");
    }

    private String extractLogTime(String regex) throws DeviceNotAvailableException {
        CommandRunner android = new CommandRunner(getDevice());
        String result = android.run("logcat", "-d", "-e", "'" + regex + "'", "-m", "1", "|", "tail", "-n", "1").trim();
        return result;
    }

    @Test
    public void testMicrodroidBootTime() throws Exception {
        final String configPath = "assets/vm_config.json"; // path inside the APK
        final String cid =
                startMicrodroid(
                        getDevice(),
                        getBuild(),
                        APK_NAME,
                        PACKAGE_NAME,
                        configPath,
                        /* debug */ true,
                        minMemorySize(),
                        Optional.of(NUM_VCPUS),
                        Optional.of(CPU_AFFINITY));
        adbConnectToMicrodroid(getDevice(), cid);
        waitForBootComplete();

        mMetrics.addTestMetric("test_avf_metric_crosvm_start_time", extractLogTime("virtualizationservice::crosvm:[[:space:]]Running"));
        mMetrics.addTestMetric("test_avf_metric_vcpu_start_time", extractLogTime("MicrodroidConsole"));
        mMetrics.addTestMetric("test_avf_metric_kernel_start_time", extractLogTime("MicrodroidConsole:[[:space:]]Starting[[:space:]]kernel"));
        mMetrics.addTestMetric("test_avf_metric_payload_start_time", extractLogTime("MicrodroidConsole:.*microdroid_manager.*executing[[:space:]]main[[:space:]]task"));

        shutdownMicrodroid(getDevice(), cid);
    }

    @Before
    public void setUp() throws Exception {
        testIfDeviceIsCapable(getDevice());

        prepareVirtualizationTestSetup(getDevice());

        getDevice().installPackage(findTestFile(APK_NAME), /* reinstall */ false);

        // clear the log
        getDevice().executeShellV2Command("logcat -c");
    }

    @After
    public void shutdown() throws Exception {
        cleanUpVirtualizationTestSetup(getDevice());

        archiveLogThenDelete(mTestLogs, getDevice(), LOG_PATH,
                "vm.log-" + mTestName.getMethodName());

        getDevice().uninstallPackage(PACKAGE_NAME);
    }
}
