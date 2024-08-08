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
package com.android.accessor.test;

import static com.google.common.truth.Truth.assertThat;

import android.os.IBinder;
import android.os.RemoteException;
import android.os.ServiceManager;

import androidx.test.ext.junit.runners.AndroidJUnit4;

import com.android.virt.accessor_demo.vm_service.IAccessorVmService;

import org.junit.Test;
import org.junit.runner.RunWith;

/** Tests Accessor implementation via Java backend. */
@RunWith(AndroidJUnit4.class)
public class VmAccessorTest {
    private static final String SERVICE_NAME =
            "com.android.virt.accessor_demo.vm_service.IAccessorVmService/default";

    private IAccessorVmService waitForService() throws RemoteException {
        IBinder binder = ServiceManager.waitForService(SERVICE_NAME);
        return IAccessorVmService.Stub.asInterface(binder);
    }

    private IAccessorVmService getService() throws RemoteException {
        IBinder binder = ServiceManager.getService(SERVICE_NAME);
        return IAccessorVmService.Stub.asInterface(binder);
    }

    private IAccessorVmService checkService() throws RemoteException {
        IBinder binder = ServiceManager.checkService(SERVICE_NAME);
        return IAccessorVmService.Stub.asInterface(binder);
    }

    @Test
    public void testWaitForService() throws RemoteException {
        IAccessorVmService service = waitForService();

        int sum = service.add(11, 22);
        assertThat(sum).isEqualTo(33);
    }

    @Test
    public void testWaitForInterface_twice() throws RemoteException {
        IAccessorVmService service1 = waitForService();
        IAccessorVmService service2 = waitForService();

        int sum1 = service1.add(11, 22);
        int sum2 = service2.add(11, 22);
        assertThat(sum1).isEqualTo(33);
        assertThat(sum2).isEqualTo(33);
    }

    @Test
    public void testWaitAndGetService() throws RemoteException {
        IAccessorVmService service1 = waitForService();
        IAccessorVmService service2 = getService();

        int sum1 = service1.add(11, 22);
        int sum2 = service2.add(11, 22);
        assertThat(sum1).isEqualTo(33);
        assertThat(sum2).isEqualTo(33);
    }

    @Test
    public void testWaitAndCheckService() throws RemoteException {
        IAccessorVmService service1 = waitForService();
        IAccessorVmService service2 = checkService();

        int sum1 = service1.add(11, 22);
        int sum2 = service2.add(11, 22);
        assertThat(sum1).isEqualTo(33);
        assertThat(sum2).isEqualTo(33);
    }
}
