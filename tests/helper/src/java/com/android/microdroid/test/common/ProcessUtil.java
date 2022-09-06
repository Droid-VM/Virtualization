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

package com.android.microdroid.test.common;

import java.io.IOException;
import java.util.HashMap;
import java.util.Map;
import java.util.function.Function;

/** This class provides process utility for both device tests and host tests. */
public final class ProcessUtil {

    // To ensures that only one object is created at a time.
    private ProcessUtil() {
    }

    /**
     * Gets metrics key and values mapping of specified process id
     */
    public static Map<String, Long> getProcSmapsRollup(int pid,
            Function<String, String> shellExecutor) throws IOException {

        String path = "/proc/" + pid + "/smaps_rollup";
        return parseMemInfo(skipFirstLine(shellExecutor.apply("cat " + path + " || true")));
    }

    /**
     * Gets process id and process name mapping of the device
     */
    public static Map<Integer, String> getProcessMap(Function<String, String> shellExecutor)
            throws IOException {

        Map<Integer, String> processMap = new HashMap<>();
        for (String ps : skipFirstLine(shellExecutor.apply("ps -Ao PID,NAME")).split("\n")) {
            // Each line is '<pid> <name>'.
            // EX : 11424 dex2oat64
            ps = ps.trim();
            // ignore blank line
            if (ps.length() == 0) {
                continue;
            }
            int space = ps.indexOf(" ");
            String pName = ps.substring(space + 1);
            int pId = Integer.parseInt(ps.substring(0, space));
            processMap.put(pId, pName);
        }

        return processMap;
    }

    private static Map<String, Long> parseMemInfo(String file) {
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

    private static String skipFirstLine(String str) {
        int index = str.indexOf("\n");
        return (index < 0) ? "" : str.substring(index + 1);
    }
}
