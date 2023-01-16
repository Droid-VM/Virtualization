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

import java.util.LinkedList;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import java.io.File;
import java.io.FileReader;
import java.io.BufferedReader;

import java.text.ParseException;

import javax.annotation.Nonnull;

/** This class provides utilities to interact with the hyp tracing subsystem */
public final class HypTracer {

    private static final String HYP_TRACING_ROOT = "/sys/kernel/tracing/hyp/";
    private static final String HYP_EVENT_LIST[] = { "enter", "exit" };
    private static final int DEFAULT_BUF_SIZE = 16*1024;

    private ITestDevice mDevice;
    private LinkedList<File> mTraces;

    private String run(String cmd) throws Exception{
        CommandRunner android = new CommandRunner(mDevice);

        return android.run(cmd);
    }

    private void setNode(String node, int val) throws Exception {
        run("echo " + val + " > " + HYP_TRACING_ROOT + node);
    }

    private LinkedList<File> pullTraces() throws Exception {
        LinkedList<File> mTraces = new LinkedList<File>();
        int nr_cpus = Integer.parseInt(run("nproc"));

        for (int cpu = 0; cpu < 8; cpu++) {
            File t = mDevice.pullFile(HYP_TRACING_ROOT + "per_cpu/cpu" + cpu + "/trace");
            if (t != null)
                mTraces.add(t);
        }

        return mTraces;
    }

    public HypTracer(@Nonnull ITestDevice device) {
        mDevice = device;
    }

    public void start(int buf_size_kb) throws Exception {
        setNode("buffer_size_kb", buf_size_kb);
        for (String evt: HYP_EVENT_LIST)
            setNode("events/" + evt + "/enable", 1);
        setNode("tracing_on", 1);
    }

    public void start() throws Exception {
        start(DEFAULT_BUF_SIZE);
    }

    public void stop() throws Exception {
        mTraces = pullTraces();
        setNode("tracing_on", 0);
    }

    public SimpleStats getDurationStats() throws Exception {
        Pattern pattern = Pattern.compile("\\[(.*)\\] ([a-z]*)");
        SimpleStats stats = new SimpleStats();

        for (File trace : mTraces) {
            BufferedReader br = new BufferedReader(new FileReader(trace));
            Double enter = -1.0;
            String l;

            while ((l = br.readLine()) != null) {
                if (l.startsWith("#"))
                    continue;

                Matcher matcher = pattern.matcher(l);
                if (!matcher.find())
                    throw new ParseException("Failed to parse hyp trace:" + l, 0);

                Double cur = Double.valueOf(matcher.group(1));
                switch (matcher.group(2)) {
                case "exit":
                    if (enter > 0.0)
                        stats.add(cur - enter);
                    break;
                case "enter":
                    enter = cur;
                    break;
                default:
                    continue;
                }
            }
        }

        return stats;
    }
}
