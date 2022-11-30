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

package com.android.vmcopy;

import com.android.tradefed.testtype.DeviceJUnit4ClassRunner;
import com.android.tradefed.testtype.junit4.BaseHostJUnit4Test;

import org.junit.Test;
import org.junit.runner.RunWith;

@RunWith(DeviceJUnit4ClassRunner.class)
public class VmCopyHostTests extends BaseHostJUnit4Test {
    private static final String SOURCE_APK_PKG = "com.android.vmcopy.source";
    private static final String SOURCE_APK_CLASS = SOURCE_APK_PKG + ".MainActivity";
    private static final String SOURCE_APK_NAME = "VmCopySourceApp.apk";
    private static final String DEST_APK_NAME = "VmCopyDestApp.apk";

    @Test
    public void vmInDestAppIsEqualToVmInSourceApp() throws Exception {
        InstallMutiple installer = new InstallMultiple();
        installer.addFile(DEST_APK_NAME);
        installer.addFile(SOURCE_APK_NAME).run();
        runDeviceTests(SOURCE_APK_PKG, SOURCE_APK_CLASS, "todo");
    }
}
