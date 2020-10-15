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

public class VsockTest extends VirtTestCase {
    private static final Long TEST_TIMEOUT = 60L;
    private static final TimeUnit TEST_TIMEOUT_UNIT = TimeUnit.SECONDS;
    private static final Long TEST_SLEEP_BETWEEN_ATTEMPTS = 1000L;

    private static final Integer GUEST_CID = 42;
    private static final Integer GUEST_PORT = 8844;

    private static final String TEST_MESSAGE = "HelloWorld";
    private static final String TEST_SERVER_PATH = "/bin/vsock_server";
    private static final String TEST_CLIENT_PATH = "bin/vsock_client";

    @Test
    public void testVsockServer() throws Exception {
        ExecutorService executor = Executors.newFixedThreadPool(2);

        prepareTest(TEST_CLIENT_PATH);

        final Future<?> vmmTask = executor.submit(new Callable<Void>() {
            @Override
            public Void call() throws Exception {
                ShellCommandResult res = runGuest(
                        GUEST_CID, TEST_SERVER_PATH, GUEST_PORT.toString(), TEST_MESSAGE);
                if (res.exitCode != EXIT_SUCCESS) {
                    throw new IllegalStateException("CrosVM failed (see execution log)");
                }
                return null;
            }
        });

        Future<?> clientTask = executor.submit(new Callable<Void>() {
            @Override
            public Void call() throws Exception {
                while (true) {
                    // Exit early if the VMM exited. No point trying to connect.
                    if (vmmTask.isDone()) {
                        return null;
                    }

                    ShellCommandResult res = VsockTest.this.runTestBinary(
                            GUEST_CID.toString(), GUEST_PORT.toString());
                    if (res.exitCode == EXIT_SUCCESS) {
                        assertEquals(TEST_MESSAGE, res.output);
                        return null;
                    }

                    Thread.sleep(TEST_SLEEP_BETWEEN_ATTEMPTS);
                }
            }
        });

        try {
            // Wait for the client to finish, or throw a TimeoutException.
            clientTask.get(TEST_TIMEOUT, TEST_TIMEOUT_UNIT);
            // Client exited before timeout. Either it was successful or the VMM
            // exited early. Get the outcome of the VMM task.
            vmmTask.get(TEST_TIMEOUT, TEST_TIMEOUT_UNIT);
        } finally {
            executor.shutdown();
            executor.awaitTermination(TEST_TIMEOUT, TEST_TIMEOUT_UNIT);
            finishTest();
        }
    }
}
