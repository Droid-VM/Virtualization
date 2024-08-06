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

package com.android.ferrochrome.testtype;

import com.android.tradefed.config.Option;
import com.android.tradefed.config.OptionClass;
import com.android.tradefed.device.DeviceNotAvailableException;
import com.android.tradefed.device.ITestDevice;
import com.android.tradefed.invoker.TestInformation;
import com.android.tradefed.log.LogUtil.CLog;
import com.android.tradefed.result.FileInputStreamSource;
import com.android.tradefed.result.ITestInvocationListener;
import com.android.tradefed.result.LogDataType;
import com.android.tradefed.testtype.binary.ExecutableHostTest;
import com.android.tradefed.util.FileUtil;

import java.io.File;
import java.io.IOException;

/** Test runner for ferrochrome with extra features */
@OptionClass(alias = "ferrochrome-host-test")
public class FerrochromeHostTest extends ExecutableHostTest {
    @Option(
            name = "collect-dir",
            description =
                    "Collect files at the specified directory in the device."
                            + " Currently assumes that everything are screenshots")
    private String mCollectDir;

    @Override
    public void run(TestInformation testInfo, ITestInvocationListener listener)
            throws DeviceNotAvailableException {
        super.run(testInfo, listener);

        if (mCollectDir != null && !mCollectDir.isEmpty()) {
            File tempDir = null;
            ITestDevice device = testInfo.getDevice();

            try {
                tempDir = FileUtil.createTempDir("ferrochrome");
                if (!device.pullDir(mCollectDir, tempDir)) {
                    CLog.w("Failed to pull requested dir=" + mCollectDir);
                    return;
                }
                File[] files = tempDir.listFiles();
                for (File file : files) {
                    try (FileInputStreamSource source = new FileInputStreamSource(file)) {
                        listener.testLog(file.getName(), LogDataType.PNG, source);
                    }
                }
            } catch (IOException e) {
                CLog.e("Failed to create temp directory");
            } finally {
                FileUtil.deleteFile(tempDir);
            }
        }
    }
}
