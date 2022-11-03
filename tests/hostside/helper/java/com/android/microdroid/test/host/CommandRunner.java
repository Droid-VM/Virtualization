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

package com.android.microdroid.test.host;

import static org.hamcrest.CoreMatchers.is;
import static org.junit.Assert.fail;
import static org.junit.Assume.assumeThat;

import com.android.tradefed.device.DeviceNotAvailableException;
import com.android.tradefed.device.ITestDevice;
import com.android.tradefed.log.LogUtil.CLog;
import com.android.tradefed.util.CommandResult;
import com.android.tradefed.util.CommandStatus;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.PipedInputStream;
import java.io.PipedOutputStream;
import java.util.Arrays;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

import javax.annotation.Nonnull;

/** A helper class to provide easy way to run commands on a test device. */
public class CommandRunner {

    /** Default timeout. 30 sec because Microdroid is extremely slow on GCE-on-CF. */
    private static final long DEFAULT_TIMEOUT = 30000;

    private ITestDevice mDevice;

    public static class BackgroundCommand {
        public final Future<CommandStatus> mStatus;
        public final BufferedReader mStdout;

        public BackgroundCommand(Future<CommandStatus> status, BufferedReader stdout) {
            mStatus = status;
            mStdout = stdout;
        }
    }

    public CommandRunner(@Nonnull ITestDevice device) {
        mDevice = device;
    }

    public ITestDevice getDevice() {
        return mDevice;
    }

    public String run(String... cmd) throws DeviceNotAvailableException {
        CommandResult result = runForResult(cmd);
        if (result.getStatus() != CommandStatus.SUCCESS) {
            fail(join(cmd) + " has failed: " + result);
        }
        return result.getStdout().trim();
    }

    public String tryRun(String... cmd) throws DeviceNotAvailableException {
        CommandResult result = runForResult(cmd);
        if (result.getStatus() == CommandStatus.SUCCESS) {
            return result.getStdout().trim();
        } else {
            CLog.d(join(cmd) + " has failed (but ok): " + result);
            return null;
        }
    }

    public String runWithTimeout(long timeoutMillis, String... cmd)
            throws DeviceNotAvailableException {
        CommandResult result =
                mDevice.executeShellV2Command(
                        join(cmd), timeoutMillis, java.util.concurrent.TimeUnit.MILLISECONDS);
        if (result.getStatus() != CommandStatus.SUCCESS) {
            fail(join(cmd) + " has failed: " + result);
        }
        return result.getStdout().trim();
    }

    public CommandResult runForResultWithTimeout(long timeoutMillis, String... cmd)
            throws DeviceNotAvailableException {
        return mDevice.executeShellV2Command(
                join(cmd), timeoutMillis, java.util.concurrent.TimeUnit.MILLISECONDS);
    }

    public CommandResult runForResult(String... cmd) throws DeviceNotAvailableException {
        return mDevice.executeShellV2Command(join(cmd));
    }

    public BackgroundCommand runInBackground(String... cmd) throws IOException {
        PipedInputStream pis = new PipedInputStream();
        final PipedOutputStream pos = new PipedOutputStream(pis);
        BufferedReader stdout = new BufferedReader(new InputStreamReader(pis));

        ExecutorService executor  = Executors.newSingleThreadExecutor();
        Future<CommandStatus> status = executor.submit(
                () -> {
                    try {
                        return mDevice.executeShellV2Command(join(cmd), pos).getStatus();
                    } catch (Exception ex) {
                        CLog.d(join(cmd) + " failed with: " + ex.getMessage());
                        return CommandStatus.EXCEPTION;
                    }
                });
        return new BackgroundCommand(status, stdout);
    }

    public void assumeSuccess(String... cmd) throws DeviceNotAvailableException {
        assumeThat(runForResult(cmd).getStatus(), is(CommandStatus.SUCCESS));
    }

    private static String join(String... strs) {
        return String.join(" ", Arrays.asList(strs));
    }
}
