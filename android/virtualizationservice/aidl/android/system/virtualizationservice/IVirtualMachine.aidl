/*
 * Copyright 2021 The Android Open Source Project
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
package android.system.virtualizationservice;

import android.system.virtualizationservice.IVirtualMachineCallback;
import android.system.virtualizationservice.VirtualMachineState;

interface IVirtualMachine {
    /**
     * Service Specific Exceptions that methods may throw.
     * All of the constants starting with ERROR_*.
     */
    /**
     * Encountered an unexpected error. This is an implementation detail and the client
     * can do nothing about it.
     */
    const int ERROR_UNEXPECTED = 0;
    /**
     * Some APIs require the VM or VM payload to be running. This error is returned
     * when the VM isn't running.
     */
    const int ERROR_VM_NOT_RUNNING = 1;
    /**
     * Requested port number is denied. Ports below 1024 are all privileged ports
     * and will not be used.
     */
    const int ERROR_PORT_NUMBER_DENIED = 2;
    /**
     * Failed to connect socket.
     */
    const int ERROR_FAILED_TO_CONNECT = 3;

    /** Get the CID allocated to the VM. */
    int getCid();

    /** Returns the current lifecycle state of the VM. */
    VirtualMachineState getState();

    /**
     * Register a Binder object to get callbacks when the state of the VM changes, such as if it
     * dies.
     */
    void registerCallback(IVirtualMachineCallback callback);

    /** Starts running the VM. */
    void start();

    /**
     * Stops this virtual machine. Stopping a virtual machine is like pulling the plug on a real
     * computer; the machine halts immediately. Software running on the virtual machine is not
     * notified with the event.
     */
    void stop();

    /** Access to the VM's memory balloon. */
    long getMemoryBalloon();
    void setMemoryBalloon(long num_bytes);

    /** Open a vsock connection to the CID of the VM on the given port. */
    ParcelFileDescriptor connectVsock(int port);

    /**
     * Create an Accessor in libbinder that will open a vsock connection
     * to the CID of the VM on the given port.
     *
     * \param instance name of the service that the accessor is responsible for.
     *        This is the same instance that we expect clients to use when trying
     *        to get the service with the ServiceManager APIs.
     *
     * \return IBinder of the IAccessor on success, or throws a service specific exception
     *         on error. See the ERROR_* values above.
     */
    IBinder createAccessorBinder(String instance, int port);

    /** Set the name of the peer end (ptsname) of the host console. */
    void setHostConsoleName(in @utf8InCpp String pathname);

    /** Suspends the VM. */
    void suspend();

    /** Resumes the suspended VM. */
    void resume();
}
