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

package com.android.pvmfw.test;

import static com.google.common.truth.Truth.assertWithMessage;

import com.android.pvmfw.test.host.Pvmfw;
import com.android.tradefed.testtype.DeviceJUnit4ClassRunner;

import org.junit.Test;
import org.junit.runner.RunWith;

/** Test test helper */
@RunWith(DeviceJUnit4ClassRunner.class)
public class PvmfwTest extends CustomPvmfwHostTestCaseBase {
    @Test
    public void testPvmfw_withConfig1_2_requiresReferenceDt() {
        try {
            new Pvmfw.Builder(getPvmfwBinFile(), getBccFile()).setVersion(1, 2).build();
            assertWithMessage("pvmfw config 1.2 must require VM reference DT").fail();
        } catch (Exception e) {
            // expected. pass-through
        }
    }
}
