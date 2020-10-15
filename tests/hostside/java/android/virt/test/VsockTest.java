/*
 * Copyright (C) 2020 The Android Open Source Project
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

package android.virt.test;

import org.junit.Test;

import java.util.concurrent.Callable;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

import com.android.tradefed.log.LogUtil.CLog;
import com.android.tradefed.util.CommandResult;
import com.android.tradefed.util.CommandStatus;

public class VsockTest extends VirtTestCase {
    private static final Long     TIMEOUT = 2L;
    private static final TimeUnit TIMEOUT_UNIT = TimeUnit.MINUTES;
    private static final Long     SLEEP_BETWEEN_ATTEMPTS_MS = 1000L;

    private static final Integer  GUEST_CID = 42;
    private static final Integer  GUEST_PORT = 45678;

    private static final String   TEST_MESSAGE = "HelloWorld";
    private static final String   SERVER_PATH = "bin/vsock_server";
    private static final String   CLIENT_TARGET = "virt_hostside_tests_vsock_client";

    @Test
    public void testVsockServer() throws Exception {
        ExecutorService executor = Executors.newFixedThreadPool(2);

        final String clientPath = getDevicePathForTestBinary(CLIENT_TARGET);
        final String clientCmd = createCommand(clientPath, GUEST_CID, GUEST_PORT);
        final String serverCmd = createCommand(SERVER_PATH, GUEST_PORT, TEST_MESSAGE);
        final String vmCmd = getVmCommand(serverCmd, GUEST_CID);

        final Future<?> vmTask = executor.submit(new Callable<Void>() {
            @Override
            public Void call() throws Exception {
                CommandResult res = getDevice().executeShellV2Command(vmCmd, TIMEOUT, TIMEOUT_UNIT);
                CLog.i(res.getStdout());
                assertEquals(CommandStatus.SUCCESS, res.getStatus());
                return null;
            }
        });

        Future<?> clientTask = executor.submit(new Callable<Void>() {
            @Override
            public Void call() throws Exception {
                CommandResult res;
                do {
                    Thread.sleep(SLEEP_BETWEEN_ATTEMPTS_MS);
                    // If the VMM exited, the test cannot succeed. Exit now.
                    assertFalse(vmTask.isDone());
                    // Run the vsock client. It returns SUCCESS if it successfully
                    // connected to the server and received a message.
                    res = getDevice().executeShellV2Command(clientCmd);
                } while (res.getStatus() != CommandStatus.SUCCESS);

                assertEquals(TEST_MESSAGE, res.getStdout().trim());
                return null;
            }
        });

        // Run `clientTask` which repeatedly attempts to connect to a vsock
        // server in the guest VM. It will throw if:
        //   * the VMM process exits,
        //   * the received message does not match `TEST_MESSAGE`, or
        //   * timeout is reached.
        try {
            clientTask.get(TIMEOUT, TIMEOUT_UNIT);
        } finally {
            executor.shutdown();
            executor.awaitTermination(TIMEOUT, TIMEOUT_UNIT);
        }
    }
}
