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

package com.android.foo.testapp;

import static com.google.common.truth.Truth.assertThat;

import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.junit.runners.JUnit4;

import android.os.ServiceManager;

@RunWith(JUnit4.class)
public class FooTest {
    private static final String TAG = "FooTest";
    private static final String SERVICE_NAME_AVF =
            "android.hardware.security.keymint.IRemotelyProvisionedComponent/avf";
    private static final String SERVICE_NAME =
            "com.android.virt.vm_attestation.testservice.IAttestationService/default";

    @Before
    public void setup() {
        // Setup
    }

    @Test
    public void serviceExists() {
        assertThat(ServiceManager.isDeclared(SERVICE_NAME_AVF)).isTrue();
        assertThat(ServiceManager.isDeclared(SERVICE_NAME)).isTrue();
    }
}
