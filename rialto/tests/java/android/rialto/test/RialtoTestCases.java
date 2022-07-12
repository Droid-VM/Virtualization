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

package android.rialto.test;

import static com.android.tradefed.testtype.DeviceJUnit4ClassRunner.TestLogData;

import static com.google.common.truth.Truth.assertWithMessage;

import android.virt.test.CommandRunner;
import android.virt.test.VirtualizationTestCaseBase;

import com.android.tradefed.testtype.DeviceJUnit4ClassRunner;

import org.json.JSONObject;
import org.junit.After;
import org.junit.Before;
import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.TestName;
import org.junit.runner.RunWith;

import java.util.regex.Matcher;
import java.util.regex.Pattern;

@RunWith(DeviceJUnit4ClassRunner.class)
public class RialtoTestCases extends VirtualizationTestCaseBase {
    private static final String PACKAGE_NAME = "com.android.rialto.test";
    public static final String RIALTO_PATH = "/data/local/tmp/virt-test/rialto.bin";
    private String mConfigPath = TEST_ROOT + "raw_config.json";
    private String mConsolePath = TEST_ROOT + "console";

    @Rule public TestLogData mTestLogs = new TestLogData();
    @Rule public TestName mTestName = new TestName();

    @Test
    public void testRialtoBootsNonProtected() throws Exception {
        boolean daemonize = false;  // vm run only wait for vm to shutdown when not daemonized
        CommandRunner android = new CommandRunner(getDevice());

        // The raw_config file used by bin/vm run
        JSONObject config = new JSONObject();
        config.put("bootloader", RIALTO_PATH);
        config.put("protectedVm", false);
        config.put("platform_version", "~1.0");

        getDevice().pushString(config.toString(), configPath);
        final String ret = android.runWithTimeout(
                60 * 1000,
                VIRT_APEX + "bin/vm run",
                daemonize ? "--daemonize" : "",
                (mConsolePath != null) ? "--console " + mConsolePath : "",
                "--log " + LOG_PATH,
                mConfigPath);
        Pattern pattern = Pattern.compile("VM shutdown cleanly");
        Matcher matcher = pattern.matcher(ret);
        assertWithMessage("Rialto Boot failed with: " + ret)
            .that(matcher.find())
            .isTrue();
        return;
    }

    @Before
    public void setUp() throws Exception {
        testIfDeviceIsCapable(getDevice());
        prepareVirtualizationTestSetup(getDevice());

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
