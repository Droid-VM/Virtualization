/*
 * Copyright (C) 2022 The Android Open Source Project
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
package com.android.compos.benchmark;

import static androidx.test.platform.app.InstrumentationRegistry.getInstrumentation;

import static com.google.common.truth.TruthJUnit.assume;

import static org.junit.Assert.assertTrue;

import android.app.Instrumentation;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.util.Log;

import com.android.microdroid.test.common.MetricsProcessor;
import com.android.microdroid.test.device.MicrodroidDeviceTestBase;

import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.junit.runners.JUnit4;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.sql.Timestamp;
import java.text.DateFormat;
import java.text.ParseException;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Date;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

@RunWith(JUnit4.class)
public class ComposBenchmark extends MicrodroidDeviceTestBase {
    private static final String TAG = "ComposBenchmark";
    private static final int BUFFER_SIZE = 1024;
    private static final int ROUND_COUNT = 2;
    private static final double NANOS_IN_SEC = 1_000_000_000.0;
    private static final String METRIC_PREFIX = "avf_perf/compos/";

    private final MetricsProcessor mMetricsProcessor = new MetricsProcessor(METRIC_PREFIX);

    private Instrumentation mInstrumentation;

    @Before
    public void setup() {
        mInstrumentation = getInstrumentation();
    }

    private void reportMetric(String name, String unit, List<? extends Number> values) {
        Map<String, Double> stats = mMetricsProcessor.computeStats(values, name, unit);
        Bundle bundle = new Bundle();
        for (Map.Entry<String, Double> entry : stats.entrySet()) {
            bundle.putDouble(entry.getKey(), entry.getValue());
        }
        mInstrumentation.sendStatus(0, bundle);
    }

    private void reportProcessMetric(String prefix,
            Map<String, List<Long>> processMemory) {
        for (Map.Entry<String, List<Long>> entry : processMemory.entrySet()) {
            reportMetric(prefix + entry.getKey(), "kb", entry.getValue());
        }
    }

    public byte[] executeCommandBlocking(String command) {
        try (
            InputStream is = new ParcelFileDescriptor.AutoCloseInputStream(
                getInstrumentation().getUiAutomation().executeShellCommand(command));
            ByteArrayOutputStream out = new ByteArrayOutputStream()
        ) {
            byte[] buf = new byte[BUFFER_SIZE];
            int length;
            while ((length = is.read(buf)) >= 0) {
                out.write(buf, 0, length);
            }
            return out.toByteArray();
        } catch (IOException e) {
            Log.e(TAG, "Error executing: " + command, e);
            return null;
        }
    }

    public String executeCommand(String command)
            throws  InterruptedException, IOException {

        getInstrumentation().getUiAutomation()
                .adoptShellPermissionIdentity();
        byte[] output = executeCommandBlocking(command);
        getInstrumentation().getUiAutomation()
                .dropShellPermissionIdentity();

        if (output == null) {
            throw new RuntimeException("Failed to run the command.");
        } else {
            String stdout = new String(output, "UTF-8");
            Log.i(TAG, "Get stdout : " + stdout);
            return stdout;
        }
    }

    private Map<String, Long> parseMemInfo(String file) {
        Map<String, Long> stats = new HashMap<>();
        file.lines().forEach(line -> {
            // Each line is '<metrics>:        <number> kB'.
            // EX : Pss_Anon:        70712 kB
            if (line.endsWith(" kB")) line = line.substring(0, line.length() - 3);

            String[] elems = line.split(":");
            stats.put(elems[0].trim(), Long.parseLong(elems[1].trim()));
        });
        return stats;
    }

    private Map<String, Long> getProcSmapsRollup(int pid) throws Exception {
        String path = "/proc/" + pid + "/smaps_rollup";
        return  parseMemInfo(skipFirstLine(executeCommand("cat " + path + " || true")));
    }

    private String skipFirstLine(String str) {
        int index = str.indexOf("\n");
        return (index < 0) ? "" : str.substring(index + 1);
    }

    private void updateProcessMemory(String processName,
            Map<String, List<Long>> processMemory) throws Exception {

        skipFirstLine(executeCommand("ps -Ao PID,NAME")).lines().forEach(ps -> {
            // Each line is '<pid> <name>'.
            // EX : 11424 dex2oat64
            ps = ps.trim();
            int space = ps.indexOf(" ");
            String pName = ps.substring(space + 1);
            int pId = Integer.parseInt(ps.substring(0, space));
            if (pName.equalsIgnoreCase(processName)) {
                try {
                    for (Map.Entry<String, Long> stat : getProcSmapsRollup(pId).entrySet()) {
                        Log.i(TAG, "Get running process " + pName + " metrics : "
                                + stat.getKey().toLowerCase() + '-' + stat.getValue());
                        processMemory.computeIfAbsent(stat.getKey().toLowerCase(),
                                k -> new ArrayList<>()).add(stat.getValue());
                    }
                } catch (Exception e) {
                    throw new RuntimeException(e);
                }
            }
        });
    }

    @Test
    public void testGuestCompileTime() throws Exception {
        assume().withMessage("Skip on CF; too slow").that(isCuttlefish()).isFalse();

        final String command = "/apex/com.android.compos/bin/composd_cmd test-compile";

        final List<Double> compileTimes = new ArrayList<>(ROUND_COUNT);
        final Map<String, List<Long>> processMemory = new HashMap<>();
        // The mapping is <memory metrics name> -> <all rounds value list>.
        // EX : pss -> [10, 20, 30, ........]

        for (int round = 0; round < ROUND_COUNT; ++round) {

            GetMetricsRunnable getMetricsRunnable = new GetMetricsRunnable("crosvm",
                    processMemory);
            Thread threadGetMetrics = new Thread(getMetricsRunnable);

            threadGetMetrics.start();

            Long compileStartTime = System.nanoTime();
            String output = executeCommand(command);
            Long compileEndTime = System.nanoTime();
            Pattern pattern = Pattern.compile("All Ok");
            Matcher matcher = pattern.matcher(output);
            assertTrue(matcher.find());
            double elapsedSec = (compileEndTime - compileStartTime) / NANOS_IN_SEC;
            Log.i(TAG, "Compile time in guest took " + elapsedSec + "s");
            getMetricsRunnable.setStop();

            Log.i(TAG, "Waits for thread finish");
            threadGetMetrics.join();
            Log.i(TAG, "Thread is finish");

            compileTimes.add(elapsedSec);
        }

        reportMetric("guest_compile_time", "s", compileTimes);

        reportProcessMetric("guest_compile_crosvm_", processMemory);
    }

    private Timestamp getLatestDex2oatSuccessTime()
            throws  InterruptedException, IOException, ParseException {

        final String command = "logcat -d -e dex2oat";
        String output = executeCommand(command);
        String latestTime = null;

        for (String line : output.split("[\r\n]+")) {
            Pattern pattern = Pattern.compile("dex2oat64: dex2oat took");
            Matcher matcher = pattern.matcher(line);
            if (matcher.find()) {
                latestTime = line.substring(0, 18);
            }
        }

        if (latestTime == null) {
            return null;
        }

        DateFormat formatter = new SimpleDateFormat("MM-dd hh:mm:ss.SSS");
        Date date = formatter.parse(latestTime);
        Timestamp timeStampDate = new Timestamp(date.getTime());

        return timeStampDate;
    }

    private class GetMetricsRunnable implements Runnable {
        private final String mProcessName;
        private Map<String, List<Long>> mProcessMemory;
        private Boolean mStop = false;

        GetMetricsRunnable(String processName, Map<String, List<Long>> processMemory) {
            this.mProcessName = processName;
            this.mProcessMemory = processMemory;
        }

        public void setStop() {
            mStop = true;
        }

        public void run() {
            while (!mStop) {
                try {
                    updateProcessMemory(mProcessName, mProcessMemory);
                    Thread.sleep(1000);
                } catch (SecurityException e) {
                    // Sometimes occur SecurityException: Calling from not trusted UID!
                    // Skip it because it will not affect result.
                    Log.w(TAG, "Get SecurityException : " + e);
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    return;
                } catch (Exception e) {
                    Log.e(TAG, "Get exception : " + e);
                    throw new RuntimeException(e);
                }
            }
        }
    }

    @Test
    public void testHostCompileTime()
            throws InterruptedException, IOException, ParseException {

        final String command = "/apex/com.android.art/bin/odrefresh --force-compile";

        final List<Double> compileTimes = new ArrayList<>(ROUND_COUNT);
        final Map<String, List<Long>> processMemory = new HashMap<>();
        // The mapping is <memory metrics name> -> <all rounds value list>.
        // EX : pss -> [10, 20, 30, ........]

        for (int round = 0; round < ROUND_COUNT; ++round) {

            GetMetricsRunnable getMetricsRunnable = new GetMetricsRunnable("dex2oat64",
                    processMemory);
            Thread threadGetMetrics = new Thread(getMetricsRunnable);

            threadGetMetrics.start();

            Timestamp beforeCompileLatestTime = getLatestDex2oatSuccessTime();
            Long compileStartTime = System.nanoTime();
            String output = executeCommand(command);
            Long compileEndTime = System.nanoTime();
            Timestamp afterCompileLatestTime = getLatestDex2oatSuccessTime();

            assertTrue(afterCompileLatestTime != null);
            assertTrue(beforeCompileLatestTime == null
                    || beforeCompileLatestTime.before(afterCompileLatestTime));

            double elapsedSec = (compileEndTime - compileStartTime) / NANOS_IN_SEC;
            Log.i(TAG, "Compile time in host took " + elapsedSec + "s");
            getMetricsRunnable.setStop();

            Log.i(TAG, "Waits for thread finish");
            threadGetMetrics.join();
            Log.i(TAG, "Thread is finish");

            compileTimes.add(elapsedSec);
        }

        reportMetric("host_compile_time", "s", compileTimes);

        reportProcessMetric("host_compile_dex2oat64_", processMemory);
    }
}
