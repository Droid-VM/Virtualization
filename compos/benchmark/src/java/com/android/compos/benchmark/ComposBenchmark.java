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
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Collectors;

@RunWith(JUnit4.class)
public class ComposBenchmark extends MicrodroidDeviceTestBase {
    private static final String TAG = "ComposBenchmark";
    private static final int BUFFER_SIZE = 1024;
    private static final int ROUND_COUNT = 5;
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

    private void reportProcMetric(String prefix,
            Map<String, List<Long>> processMemoryValues) {
        for (Map.Entry<String, List<Long>> entry : processMemoryValues.entrySet()) {
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

    private static class ProcessInfo {
        private final String mName;
        private final int mPid;

        ProcessInfo(String name, int pid) {
            mName = name;
            mPid = pid;
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

    private List<ProcessInfo> getRunningProcessesList() throws Exception {
        List<ProcessInfo> list = new ArrayList<ProcessInfo>();
        skipFirstLine(executeCommand("ps -Ao PID,NAME")).lines().forEach(ps -> {
            // Each line is '<pid> <name>'.
            // EX : 11424 dex2oat64
            ps = ps.trim();
            int space = ps.indexOf(" ");
            list.add(new ProcessInfo(
                    ps.substring(space + 1),
                    Integer.parseInt(ps.substring(0, space))));
        });

        return list;
    }

    private void updateProcessMemoryValues(String processName,
            Map<String, List<Long>> processMemoryValues) throws Exception {

        List<ProcessInfo> processList = getRunningProcessesList().stream()
                .filter(p -> p.mName.equalsIgnoreCase(processName))
                .collect(Collectors.toList());
        for (ProcessInfo proc : processList) {
            for (Map.Entry<String, Long> stat : getProcSmapsRollup(proc.mPid).entrySet()) {
                String name = stat.getKey().toLowerCase();

                Log.i(TAG, "Get running process " + proc.mName + " metrics : " + name
                        + '-' + stat.getValue());

                if (!processMemoryValues.containsKey(name)) {
                    List<Long> valueList = new ArrayList<>();
                    valueList.add(stat.getValue());
                    processMemoryValues.put(name, valueList);
                } else {
                    processMemoryValues.get(name).add(stat.getValue());
                }
            }
        }
    }

    @Test
    public void testGuestCompileTime() throws Exception {
        assume().withMessage("Skip on CF; too slow").that(isCuttlefish()).isFalse();

        final String command = "/apex/com.android.compos/bin/composd_cmd test-compile";

        final List<Double> compileTimeArray = new ArrayList<>(ROUND_COUNT);
        final Map<String, List<Long>> processMemoryValues = new HashMap<>();
        // The mapping is <memory metrics name> -> <all rounds value list>.
        // EX : pss -> [10, 20, 30, ........]

        for (int round = 0; round < ROUND_COUNT; ++round) {

            AtomicBoolean isCompilationFinish = new AtomicBoolean(false);

            Thread threadGetMetrics = new Thread(new GetMetricsRunnable("crosvm",
                    processMemoryValues, isCompilationFinish));

            threadGetMetrics.start();

            Long compileStartTime = System.nanoTime();
            String output = executeCommand(command);
            Long compileEndTime = System.nanoTime();
            Pattern pattern = Pattern.compile("All Ok");
            Matcher matcher = pattern.matcher(output);
            assertTrue(matcher.find());
            double elapsedSec = (compileEndTime - compileStartTime) / NANOS_IN_SEC;
            Log.i(TAG, "Compile time in guest took " + elapsedSec + "s");
            isCompilationFinish.set(true);

            Log.i(TAG, "Waits for thread finish");
            threadGetMetrics.join();
            Log.i(TAG, "Thread is finish");

            compileTimeArray.add(elapsedSec);
        }

        reportMetric("guest_compile_time", "s", compileTimeArray);

        reportProcMetric("guest_compile_crosvm_", processMemoryValues);
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
        private String mProcessName;
        private Map<String, List<Long>> mProcessMemoryValues;
        private AtomicBoolean mIsCompilationFinish;

        GetMetricsRunnable(String processName, Map<String, List<Long>> processMemoryValues,
                AtomicBoolean isCompilationFinish) {
            this.mProcessName = processName;
            this.mProcessMemoryValues = processMemoryValues;
            this.mIsCompilationFinish = isCompilationFinish;
        }

        public void run() {
            while (true) {
                try {
                    updateProcessMemoryValues(mProcessName, mProcessMemoryValues);
                    Thread.sleep(1000);
                } catch (Exception e) {
                    Log.e(TAG, "Get exception : " + e);
                }

                if (mIsCompilationFinish.get()) {
                    break;
                }
            }
        }
    }

    @Test
    public void testHostCompileTime()
            throws InterruptedException, IOException, ParseException {

        final String command = "/apex/com.android.art/bin/odrefresh --force-compile";

        final List<Double> compileTimeArray = new ArrayList<>(ROUND_COUNT);
        final Map<String, List<Long>> processMemoryValues = new HashMap<>();
        // The mapping is <memory metrics name> -> <all rounds value list>.
        // EX : pss -> [10, 20, 30, ........]

        for (int round = 0; round < ROUND_COUNT; ++round) {

            AtomicBoolean isCompilationFinish = new AtomicBoolean(false);

            Thread threadGetMetrics = new Thread(new GetMetricsRunnable("dex2oat64",
                    processMemoryValues, isCompilationFinish));

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
            isCompilationFinish.set(true);

            Log.i(TAG, "Waits for thread finish");
            threadGetMetrics.join();
            Log.i(TAG, "Thread is finish");

            compileTimeArray.add(elapsedSec);
        }

        reportMetric("host_compile_time", "s", compileTimeArray);

        reportProcMetric("host_compile_dex2oat64_", processMemoryValues);
    }
}
