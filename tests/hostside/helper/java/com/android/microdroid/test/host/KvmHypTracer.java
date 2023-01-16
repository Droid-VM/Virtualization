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

package com.android.microdroid.test.host;

import com.android.tradefed.util.SimpleStats;
import com.android.tradefed.device.ITestDevice;
import com.android.microdroid.test.host.CommandRunner;

import java.util.ArrayList;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import java.io.File;
import java.io.FileReader;
import java.io.BufferedReader;

import java.text.ParseException;

import javax.annotation.Nonnull;

/** This class provides utilities to interact with the hyp tracing subsystem */
public final class KvmHypTracer {

    private static final String HYP_TRACING_ROOT = "/sys/kernel/tracing/hyp/";
    private static final String HYP_EVENTS[] = { "enter", "exit" };
    private static final int DEFAULT_BUF_SIZE_KB = 32 * 1024;
    private static final Pattern HEADER_PATTERN = Pattern.compile(
            "^# entries-in-buffer/entries-written: ([0-9]*)/([0-9]*)");
    private static final Pattern EVENT_PATTERN = Pattern.compile(
            "^\\[ *([0-9]*\\.[0-9]*) *\\] (" + String.join("|", HYP_EVENTS) + ") (.*)");

    private final CommandRunner mRunner;
    private final ITestDevice mDevice;

    private ArrayList<File> mTraces;

    private void setNode(String node, int val) throws Exception {
        mRunner.run("echo " + val + " > " + HYP_TRACING_ROOT + node);
    }

    private ArrayList<File> pullTraces() throws Exception {
        mTraces = new ArrayList<File>();
        int nr_cpus = Integer.parseInt(mRunner.run("nproc"));

        for (int cpu = 0; cpu < nr_cpus; cpu++) {
            File t = mDevice.pullFile(HYP_TRACING_ROOT + "per_cpu/cpu" + cpu + "/trace");
            if (t != null)
                mTraces.add(t);
        }

        return mTraces;
    }

    private int getDroppedEvents(String header) throws Exception {
        Matcher matcher = HEADER_PATTERN.matcher(header);

        if (!matcher.find())
            throw new ParseException("Failed to parse hyp trace header" + header, 0);

        int inBuffer = Integer.parseInt(matcher.group(1));
        int written = Integer.parseInt(matcher.group(2));

        return written - inBuffer;
    }

    public static boolean isSupported(ITestDevice device) throws Exception {
        for (String event: HYP_EVENTS) {
            if (!device.doesFileExist(HYP_TRACING_ROOT + "events/" + event + "/enable"))
                return false;
        }
        return true;
    }

    public KvmHypTracer(@Nonnull ITestDevice device) {
        mDevice = device;
        mRunner = new CommandRunner(mDevice);
        mTraces = new ArrayList<File>();
    }

    public void start(int bufSizeKb) throws Exception {
        setNode("tracing_on", 0);
        mRunner.run("echo 0 | tee " + HYP_TRACING_ROOT + "events/*/enable");
        for (String event: HYP_EVENTS)
            setNode("events/" + event + "/enable", 1);
        setNode("buffer_size_kb", bufSizeKb);
        setNode("tracing_on", 1);
    }

    public void start() throws Exception {
        start(DEFAULT_BUF_SIZE_KB);
    }

    public void stop() throws Exception {
        setNode("tracing_on", 0);
        mTraces = pullTraces();
    }

    public SimpleStats getDurationStats() throws Exception {
        SimpleStats stats = new SimpleStats();

        for (File trace: mTraces) {
            BufferedReader br = new BufferedReader(new FileReader(trace));
            String l, prev = "";
            double enter = 0.0;

            if (getDroppedEvents(br.readLine()) > 0)
                throw new OutOfMemoryError("Dropped hyp events, buffer too small");

            while ((l = br.readLine()) != null) {
                Matcher matcher = EVENT_PATTERN.matcher(l);
                if (!matcher.find())
                    throw new ParseException("Failed to parse hyp trace:" + l, 0);

                double cur = Double.parseDouble(matcher.group(1));
                String event = matcher.group(2);
                if (event.equals(prev))
                    throw new ParseException("Hyp event found twice in a row: " + event, 0);

                switch (event) {
                case "exit":
                    if (prev.equals("enter"))
                        stats.add(cur - enter);
                    break;
                case "enter":
                    enter = cur;
                    break;
                default:
                    continue;
                }
                prev = event;
            }
        }

        return stats;
    }
}
