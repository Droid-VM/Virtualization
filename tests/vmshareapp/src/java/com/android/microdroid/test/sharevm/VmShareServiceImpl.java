/*
 * Copyright (C) 2023 The Android Open Source Project
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

package com.android.microdroid.test.sharevm;

import android.app.Service;
import android.content.Intent;
import android.os.IBinder;
import android.os.RemoteException;
import android.system.virtualmachine.VirtualMachine;
import android.system.virtualmachine.VirtualMachineCallback;
import android.system.virtualmachine.VirtualMachineDescriptor;
import android.system.virtualmachine.VirtualMachineManager;
import android.util.Log;

import com.android.microdroid.test.vmshare.IVmShareTestService;
import com.android.microdroid.testservice.ITestService;

import java.util.UUID;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

/** TODO(ioffe): document */
public class VmShareServiceImpl extends Service {

    private static final String TAG = "VmShareApp";

    private IVmShareTestService.Stub mBinder;

    @Override
    public void onCreate() {
        mBinder = new BinderImpl();
    }

    @Override
    public IBinder onBind(Intent intent) {
        Log.i(TAG, "onBind " + intent + " binder = " + mBinder);
        return mBinder;
    }

    private VirtualMachine mVirtualMachine;

    public IBinder startVm(VirtualMachineDescriptor vmDesc) throws Exception {
        VirtualMachineManager vmm = getSystemService(VirtualMachineManager.class);

        // Add random uuid to make sure that different tests that bind to this service don't trip
        // over each other.
        String vmName = "imported_vm" + UUID.randomUUID();

        final CountDownLatch latch = new CountDownLatch(1);
        VirtualMachineCallback callback =
                new VirtualMachineCallback() {

                    @Override
                    public void onPayloadStarted(VirtualMachine vm) {
                        // Ignored
                    }

                    @Override
                    public void onPayloadReady(VirtualMachine vm) {
                        latch.countDown();
                    }

                    @Override
                    public void onPayloadFinished(VirtualMachine vm, int exitCode) {
                        // Ignored
                    }

                    @Override
                    public void onError(VirtualMachine vm, int errorCode, String message) {
                        throw new RuntimeException(
                                "VM failed with error " + errorCode + " : " + message);
                    }

                    @Override
                    public void onStopped(VirtualMachine vm, int reason) {
                        // Ignored
                    }
                };

        mVirtualMachine = vmm.importFromDescriptor(vmName, vmDesc);
        mVirtualMachine.setCallback(getMainExecutor(), callback);

        Log.i(TAG, "Starting VM " + vmName);
        mVirtualMachine.run();
        if (!latch.await(1, TimeUnit.MINUTES)) {
            throw new TimeoutException("Timed out starting VM");
        }

        Log.i(
                TAG,
                "Payload is ready, connecting to the vsock service at port "
                        + ITestService.SERVICE_PORT);
        ITestService testService =
                ITestService.Stub.asInterface(
                        mVirtualMachine.connectToVsockServer(ITestService.SERVICE_PORT));
        return new RemoteTestServiceDelegate(testService);
    }

    final class BinderImpl extends IVmShareTestService.Stub {

        @Override
        public IBinder startVm(VirtualMachineDescriptor vmDesc) {
            Log.i(TAG, "startVm binder call received");
            try {
                return VmShareServiceImpl.this.startVm(vmDesc);
            } catch (Exception e) {
                Log.e(TAG, "Failed to startVm", e);
                throw new IllegalStateException("Failed to startVm", e);
            }
        }
    }

    final class RemoteTestServiceDelegate extends ITestService.Stub {

        private final ITestService mServiceInVm;

        private RemoteTestServiceDelegate(ITestService serviceInVm) {
            mServiceInVm = serviceInVm;
        }

        @Override
        public int addInteger(int a, int b) throws RemoteException {
            return mServiceInVm.addInteger(a, b);
        }

        @Override
        public String readProperty(String prop) throws RemoteException {
            throw new UnsupportedOperationException("Not supported");
        }

        @Override
        public byte[] insecurelyExposeVmInstanceSecret() throws RemoteException {
            throw new UnsupportedOperationException("Not supported");
        }

        @Override
        public byte[] insecurelyExposeAttestationCdi() throws RemoteException {
            throw new UnsupportedOperationException("Not supported");
        }

        @Override
        public byte[] getBcc() throws RemoteException {
            throw new UnsupportedOperationException("Not supported");
        }

        @Override
        public String getApkContentsPath() throws RemoteException {
            throw new UnsupportedOperationException("Not supported");
        }

        @Override
        public String getEncryptedStoragePath() throws RemoteException {
            throw new UnsupportedOperationException("Not supported");
        }

        @Override
        public void runEchoReverseServer() throws RemoteException {
            throw new UnsupportedOperationException("Not supported");
        }

        @Override
        public String[] getEffectiveCapabilities() throws RemoteException {
            throw new UnsupportedOperationException("Not supported");
        }

        @Override
        public void writeToFile(String content, String path) throws RemoteException {
            throw new UnsupportedOperationException("Not supported");
        }

        @Override
        public String readFromFile(String path) throws RemoteException {
            return mServiceInVm.readFromFile(path);
        }

        @Override
        public void quit() throws RemoteException {
            throw new UnsupportedOperationException("Not supported");
        }
    }
}
