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

package com.android.virtualizationmanager.fsfdt.test;

import static com.google.common.truth.Truth.assertWithMessage;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import com.android.compatibility.common.tradefed.build.CompatibilityBuildHelper;
import com.android.tradefed.testtype.DeviceJUnit4ClassRunner;
import com.android.tradefed.testtype.junit4.BaseHostJUnit4Test;
import com.android.tradefed.util.CommandResult;
import com.android.tradefed.util.CommandStatus;
import com.android.tradefed.util.RunUtil;

import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;

import java.util.concurrent.TimeUnit;

import java.io.File;

// Test for FsFdtHost test
@RunWith(DeviceJUnit4ClassRunner.class)
public class FsFdtHostTest extends BaseHostJUnit4Test {
    private static final int TIMEOUT_MS = 1000;

    // Keep this synced with AndroidTest.xml
    // Constants for devices
    @NonNull private static final String TEST_ROOT = "/data/local/tmp/fsfdt_test/";
    @NonNull private static final String FS_FDT_PATH = TEST_ROOT + "data/fs";
    @NonNull private static final String FS_FDT_TOOL_NAME = TEST_ROOT + "fsfdt";

    // Constants on host
    @NonNull private static final String EXPECTED_FDT_NAME = "expected.dts";
    @NonNull private static final String DT_DIFF_TOOL_NAME = "dtdiff";

    @Nullable File mExpectedFdtFile;

    @Before
    public void setUp() throws Exception {
        CompatibilityBuildHelper helper = new CompatibilityBuildHelper(getBuild());
        mExpectedFdtFile = helper.getTestFile(EXPECTED_FDT_NAME);
    }

    @Test
    public void testFsFdt() throws Exception {
        String testGeneratedFdtPath = TEST_ROOT + "generated.dtb";
        fsToFdt(FS_FDT_PATH, testGeneratedFdtPath);

        File testGeneratedFile = getDevice().pullFile(testGeneratedFdtPath);
        assertFdtEquals(mExpectedFdtFile, testGeneratedFile);
    }

    private void fsToFdt(String fsPath, String fdtPath) throws Exception {
        CommandResult result =
                getDevice()
                        .executeShellV2Command(
                                String.join(" ", FS_FDT_TOOL_NAME, fsPath, fdtPath),
                                TIMEOUT_MS,
                                TimeUnit.MILLISECONDS);
        assertWithMessage(result.toString())
                .that(result.getStatus())
                .isEqualTo(CommandStatus.SUCCESS);
    }

    private void assertFdtEquals(File file1, File file2) throws Exception {
        CommandResult result =
                RunUtil.getDefault()
                        .runTimedCmd(
                                TIMEOUT_MS, DT_DIFF_TOOL_NAME, file1.getPath(), file2.getPath());
        assertWithMessage(result.toString())
                .that(result.getStatus())
                .isEqualTo(CommandStatus.SUCCESS);
    }
}
