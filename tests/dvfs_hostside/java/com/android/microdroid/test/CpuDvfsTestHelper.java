/*
 * Copyright (C) 2024 Google LLC.
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
package android.avf.test;

import com.android.tradefed.device.ITestDevice;
import com.android.tradefed.util.CommandResult;
import com.android.tradefed.util.CommandStatus;

import org.junit.Assert;

import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.TimeUnit;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import javax.annotation.Nonnull;

public class CpuDvfsTestHelper {
    public static final String BASE_DIR = "/data/local/tmp/DVFSTests/";

    private static final long CMD_TIMEOUT_SECONDS = 300;
    private static final long TRACE_CAPTURE_DELAY = 2000;
    private static final String LOG_DIR = "/data/local/tmp/DVFSTestLogs/";
    private static final String RTAPP = BASE_DIR + "rt-app";
    private static final String RUN_RTAPP = RTAPP + " -l 0";
    private static final String RTAPP_CONFIGS = BASE_DIR + "rtapp_configs/";
    private static final String TRACEFS = "/sys/kernel/tracing/";
    private static final String TRACEFS_CPUFREQ_EN = TRACEFS + "events/power/cpu_frequency/enable";
    private static final String TRACEFS_SCHED_EN = TRACEFS + "events/sched/sched_switch/enable";
    private static final String TRACEFS_EN = TRACEFS + "tracing_on";
    private static final String TRACEFS_TRACE = TRACEFS + "trace";
    private static final String TRACEFS_BUFFER = TRACEFS + "buffer_size_kb";
    private static final String TRACEFS_BUF_SIZE_KB = "16384";
    private static final String SAVED_TRACE = BASE_DIR + "ftrace.txt";
    private static final String PERFETTO_TRACE_PROC = BASE_DIR + "trace_processor_shell";
    private static final String DVFS_LAT_PATTERN = "\\d+";
    private final double mConstNumStdRemove = 2.0; // 95% of values in Gaussian distribution

    private Map<String, String> mCGroupMap = new HashMap<String, String>();

    private ITestDevice mDevice;

    // constant maps CPU cluster to index of sorted capacity
    public static final Map<String, Integer> sCapacityIdxMap = new HashMap<String, Integer>();

    static {
        sCapacityIdxMap.put("little", 0);
        sCapacityIdxMap.put("middle", 1);
        sCapacityIdxMap.put("big", 2);
    }

    // constant map clusters to appropriate mask for affinity of RT-APP
    public static final Map<String, String> sClusterAffineMap = new HashMap<String, String>();

    static {
        sClusterAffineMap.put("little", "8"); // 0x08 masks CPU3 on little cluster
        sClusterAffineMap.put("middle", "10"); // 0x10 masks CPU4 on middle cluster
        sClusterAffineMap.put("big", "40"); // 0x40 masks CPU6 on big cluster
    }

    // constant map clusters for affining Cgroups. Affine to the inverse of current CPU to
    // intentionally move tasks away from the current CPU
    public static final Map<String, String> sCGroupInverseAffineMap = new HashMap<String, String>();

    static {
        sCGroupInverseAffineMap.put("little", "4-7");
        sCGroupInverseAffineMap.put("middle", "0-3,6-7");
        sCGroupInverseAffineMap.put("big", "0-5");
    }

    public CpuDvfsTestHelper(@Nonnull ITestDevice device) {
        mDevice = device;
    }

    public ITestDevice getDevice() {
        return mDevice;
    }

    public void prepDevice() throws Exception {
        String[] cmdList = {
            "echo > " + TRACEFS_TRACE,
            "echo " + TRACEFS_BUF_SIZE_KB + " > " + TRACEFS_BUFFER,
            "echo 1 > " + TRACEFS_SCHED_EN,
            "echo 1 > " + TRACEFS_CPUFREQ_EN,
            "echo 1 > " + TRACEFS_EN,
            "mkdir -p " + LOG_DIR,
            "stop",
            "stop hwservicemanager",
            "stop servicemanager",
            "stop audioserver",
            "stop cameraserver",
            "stop media"
        };
        for (int idx = 0; idx < cmdList.length; idx++) {
            Assert.assertEquals(
                    "Failed to execute cmd: " + cmdList[idx],
                    CommandStatus.SUCCESS,
                    mDevice.executeShellV2Command(cmdList[idx]).getStatus());
        }
        getCurrCGroups();
    }

    public void prepMicrodroid() throws Exception {
        String[] cmdList = {
            "echo > " + TRACEFS_TRACE,
            "echo " + TRACEFS_BUF_SIZE_KB + " > " + TRACEFS_BUFFER,
            "echo 1 > " + TRACEFS_SCHED_EN,
            "echo 1 > " + TRACEFS_CPUFREQ_EN,
            "echo 1 > " + TRACEFS_EN,
            "mkdir -p " + LOG_DIR,
            "chmod +x " + PERFETTO_TRACE_PROC,
            "chmod +x " + RTAPP,
        };
        for (int idx = 0; idx < cmdList.length; idx++) {
            Assert.assertEquals(
                    "Failed to execute cmd: " + cmdList[idx],
                    CommandStatus.SUCCESS,
                    mDevice.executeShellV2Command(cmdList[idx]).getStatus());
        }
    }

    public void restoreDevice() throws Exception {
        mDevice.executeShellV2Command("adb reboot userspace");
        mDevice.executeShellV2Command("adb wait-for-device");
    }

    /**
     * Queries device for device-specific information, updates SQL query, and then runs rt-app
     * before averaging results of output for specific test case
     */
    public double runRTApp(String config, String query, int iterations) throws Exception {
        CommandResult res;
        double latAvg = 0.0;
        double[] nominalArray = new double[iterations];
        String affineMaskParam = "";

        // check that config files exist on device before running tests
        Assert.assertTrue(
                "Failed to find config on device: " + config,
                doesFileExistOnDevice(RTAPP_CONFIGS + config));
        Assert.assertTrue(
                "Failed to find query on device: " + query,
                doesFileExistOnDevice(RTAPP_CONFIGS + query));
        updateQuery(query);
        updateCGroups(query);
        for (int i = 0; i < iterations; i++) {
            mDevice.executeShellV2Command("echo > " + TRACEFS_TRACE);
            affineMaskParam = getAffineMaskForRTApp(config);
            res =
                    mDevice.executeShellV2Command(
                            affineMaskParam + RUN_RTAPP + " " + RTAPP_CONFIGS + config,
                            CMD_TIMEOUT_SECONDS,
                            TimeUnit.SECONDS,
                            0);
            Assert.assertEquals(
                    "Failed to run rt-app:" + config + " iter:" + String.valueOf(i),
                    CommandStatus.SUCCESS,
                    res.getStatus());
            mDevice.executeShellV2Command("cat " + TRACEFS_TRACE + " > " + SAVED_TRACE);
            Thread.sleep(TRACE_CAPTURE_DELAY);
            res =
                    mDevice.executeShellV2Command(
                            PERFETTO_TRACE_PROC
                                    + " -q "
                                    + RTAPP_CONFIGS
                                    + query
                                    + " "
                                    + SAVED_TRACE,
                            CMD_TIMEOUT_SECONDS,
                            TimeUnit.SECONDS,
                            0);
            Pattern pattern = Pattern.compile(DVFS_LAT_PATTERN);
            Matcher matcher = pattern.matcher(res.getStdout());
            saveLog(query, i);
            String temp = null;
            Assert.assertTrue(
                    config + " has invalid Perfetto output at " + i + "/" + iterations + " index",
                    matcher.find());
            temp = matcher.group(0);
            nominalArray[i] = Double.parseDouble(temp);
        }

        return averageWithoutOutliers(nominalArray);
    }

    /** Get folder structure, irrespective of if cgroupsv1 or cgroupsv2 is supported */
    private HashMap<String, String> getCGroupPath() throws Exception {
        HashMap<String, String> cGroupMap = new HashMap<>();

        CommandResult res = mDevice.executeShellV2Command("mount | grep cpuset");
        Assert.assertEquals(
                "Failed to find cgroups path. Stderr: " + res.getStderr(),
                CommandStatus.SUCCESS,
                res.getStatus());

        // split by whitespace and get cgroups path and cgroups version
        String[] cpuSetInfo = res.getStdout().trim().split("\\s+");
        String cGroupVersion = cpuSetInfo[0];
        String cGroupPath = cpuSetInfo[2];

        res = mDevice.executeShellV2Command("ls " + cGroupPath);
        Assert.assertEquals(
                "Failed to get CGroups from device. Stderr: " + res.getStderr(),
                CommandStatus.SUCCESS,
                res.getStatus());

        // suffix is dependent on cgroups v1 or v2
        cGroupMap.put("prefix", cGroupPath);
        cGroupMap.put("suffix", cGroupVersion.equals("cgroup2") ? "cpuset.cpus" : "cpus");

        return cGroupMap;
    }

    /** Get original CGroups placement to reset after tests are finished */
    private void getCurrCGroups() throws Exception {
        CommandResult res;
        HashMap<String, String> cGroupMap = getCGroupPath();
        String cGroupPath = cGroupMap.get("prefix");
        String cpuSetDir = cGroupMap.get("suffix");

        // store list of original CGroup values so it can be reset after tests
        res = mDevice.executeShellV2Command("ls " + cGroupPath);
        for (String path : res.getStdout().split(System.lineSeparator())) {
            String cpuSetPath = cGroupPath + "/" + path + "/" + cpuSetDir;

            res = mDevice.executeShellV2Command("cat " + cpuSetPath);
            if (CommandStatus.SUCCESS == res.getStatus() && res.getStdout().length() > 0) {
                mCGroupMap.put(cpuSetPath, res.getStdout());
            }
        }
    }

    /** Reset CGroups to original values */
    public void resetCGroups() throws Exception {
        CommandResult res;

        for (Map.Entry<String, String> entry : mCGroupMap.entrySet()) {
            String path = entry.getKey();
            String origAffine = entry.getValue();

            res = mDevice.executeShellV2Command("echo " + origAffine + " >" + path);
            Assert.assertEquals(
                    "Failed to reset CGroups on device. Stderr: " + res.getStderr(),
                    CommandStatus.SUCCESS,
                    res.getStatus());
        }
    }

    /** Update CGroups to new values; Will affine to CPUs outside of CPU under test's cluster */
    private void updateCGroups(String query) throws Exception {
        CommandResult res;
        String newAffine = null;

        // get new CGroup corresponding to little, middle, or big
        for (Map.Entry<String, String> entry : sCGroupInverseAffineMap.entrySet()) {
            if (query.toUpperCase().contains(entry.getKey().toUpperCase())) {
                newAffine = entry.getValue();
                break;
            }
        }

        Assert.assertTrue(
                "Failed to find new affinity cgroup for query: " + query, newAffine != null);
        for (Map.Entry<String, String> entry : mCGroupMap.entrySet()) {
            String path = entry.getKey();

            res = mDevice.executeShellV2Command("echo " + newAffine + " >" + path);
            Assert.assertEquals(
                    "Failed to affine "
                            + newAffine
                            + " to "
                            + path
                            + " for "
                            + query
                            + "err: "
                            + res.getStderr(),
                    CommandStatus.SUCCESS,
                    res.getStatus());
        }
    }

    /** Includes test name info in ftrace log name and places it in the appropriate directory */
    private void saveLog(String query, int iter) throws Exception {
        String baseName = query.substring(0, query.lastIndexOf('.'));
        String testNamePath = LOG_DIR + "perfetto_" + baseName + "_test" + iter + ".log";
        mDevice.executeShellV2Command("mv " + SAVED_TRACE + " " + testNamePath);
    }

    /** Update the generic query file with device specific information */
    private void updateQuery(String query) throws Exception {
        int maxCpuId, capacity, selectedCPU;
        HashMap<Integer, Integer> cpuCapacity = new HashMap<Integer, Integer>();

        // find max CPU ID, which indirectly tells number of CPU on device
        maxCpuId = maxCpuId();
        Assert.assertTrue(
                "Could not find max number of CPUs on device: " + maxCpuId, maxCpuId >= 0);

        // find lowest CPU # per capacity
        for (int i = maxCpuId; i >= 0; i--) {
            // loop backwards so lowest CPU # will be final key-value pair
            capacity = getCPUCapacity(i);
            if (capacity != 0) {
                cpuCapacity.put(capacity, i);
            }
        }
        Assert.assertTrue(
                "Number of CPU clusters of device: "
                        + cpuCapacity.size()
                        + " does not match predefined cluster size: "
                        + sClusterAffineMap.size(),
                cpuCapacity.size() == sClusterAffineMap.size());

        // get CPU ID corresponding to the query
        selectedCPU = selectCPU(query, cpuCapacity);
        Assert.assertTrue(
                "Can't find CPU corresponding to capacity: " + cpuCapacity,
                0 <= selectedCPU && selectedCPU <= maxCpuId);

        // concatenate device specific freq with generic query
        Assert.assertTrue(
                "Failed to replace text for CPU: " + selectedCPU + " query: " + query,
                concatQuery(selectedCPU, query));
    }

    /** Concatenate constants table with device specific info to query file */
    private boolean concatQuery(int CPU, String query) throws Exception {
        CommandResult res;
        String queryConstantsStr, prependConstQueryStr;
        String absQueryPath = RTAPP_CONFIGS + query;

        float scaling = 0f;
        if (query.matches(".*\\d.*")) {
            scaling = Integer.parseInt(query.replaceAll("[^\\d]", "")) / 100f;
        }
        int fractionalFreq = (int) (getCpuFreq(CPU, true) * scaling);
        int fminFreq = getCpuFreq(CPU, false);
        int fmaxFreq = getCpuFreq(CPU, true);
        Assert.assertTrue(
                "Failed to get min CPU freq: " + fminFreq + "  for CPU" + CPU, fminFreq > 0);
        Assert.assertTrue(
                "Failed to get max CPU freq: " + fmaxFreq + "  for CPU" + CPU, fmaxFreq > 0);

        queryConstantsStr =
                "with constants as ( SELECT "
                        + fminFreq
                        + " as fmin, "
                        + fmaxFreq
                        + " as fmax, "
                        + fractionalFreq
                        + " as fracFreq),";
        prependConstQueryStr = "sed -i '1i" + queryConstantsStr + "' " + absQueryPath;
        res = mDevice.executeShellV2Command(prependConstQueryStr);

        return CommandStatus.SUCCESS == res.getStatus();
    }

    /** Get FMax or FMin of CPU in MHz */
    private int getCpuFreq(int CPU, boolean getFmax) throws Exception {
        CommandResult res;
        String stdout;
        int freq = 0;
        String freqFile = getFmax ? "/cpuinfo_max_freq" : "/cpuinfo_min_freq";
        res =
                mDevice.executeShellV2Command(
                        "cat /sys/devices/system/cpu/cpufreq/policy" + CPU + freqFile);
        if (CommandStatus.SUCCESS == res.getStatus() && !res.getStdout().isEmpty()) {
            stdout = res.getStdout().trim();
            freq = Integer.parseInt(stdout);
        }
        return freq;
    }

    /** Get CPU corresponding to the query; e.g. CPU0 for little, CPU4 for middle */
    private int selectCPU(String query, HashMap<Integer, Integer> cpuCapacity) throws Exception {
        // sort the capacitys; creates a sorted list from keys
        // {160: 0, 520: 4, 1000: 6}
        ArrayList<Integer> sortedKeys = new ArrayList<Integer>(cpuCapacity.keySet());
        Collections.sort(sortedKeys);
        int selectCpuCapacity = 0;

        // get capacity corresponding to little, middle, or big
        for (Map.Entry<String, Integer> entry : sCapacityIdxMap.entrySet()) {
            if (query.toUpperCase().contains(entry.getKey().toUpperCase())) {
                selectCpuCapacity = sortedKeys.get(entry.getValue());
                break;
            }
        }

        // get CPU ID from capacity
        return selectCpuCapacity != 0 ? cpuCapacity.get(selectCpuCapacity) : -1;
    }

    /** Get capacity of CPU based on CPU ID */
    private int getCPUCapacity(int CPU) throws Exception {
        CommandResult res;
        String stdout;
        int capacity = 0;

        res =
                mDevice.executeShellV2Command(
                        "cat /sys/devices/system/cpu/cpu" + CPU + "/cpu_capacity");
        if (CommandStatus.SUCCESS == res.getStatus() && !res.getStdout().isEmpty()) {
            stdout = res.getStdout().trim();
            capacity = Integer.parseInt(stdout);
        }
        return capacity;
    }

    /** Get max CPU ID on device */
    private int maxCpuId() throws Exception {
        CommandResult res;
        String stdout;
        int cpuId = -1;

        res = mDevice.executeShellV2Command("cat /sys/devices/system/cpu/present");
        if (CommandStatus.SUCCESS == res.getStatus() && !res.getStdout().isEmpty()) {
            stdout = res.getStdout().trim();
            cpuId = Integer.parseInt(stdout.substring(stdout.length() - 1));
        }
        return cpuId;
    }

    /** Checks if absolute path of file exists on device */
    private boolean doesFileExistOnDevice(String absPath) throws Exception {
        CommandResult res = mDevice.executeShellV2Command("test -f " + absPath);

        return res.getStatus() == CommandStatus.SUCCESS;
    }

    /** Returns the average of an array */
    private double average(double[] array) throws Exception {
        double sum = 0.0;
        for (int i = 0; i < array.length; i++) {
            sum += array[i];
        }
        return sum / array.length;
    }

    /** Returns the standard deviation of the array */
    private double stdev(double[] array) throws Exception {
        double average = average(array);
        double squaredSum = 0.0;
        for (int i = 0; i < array.length; i++) {
            squaredSum += Math.pow(average - array[i], 2);
        }
        return Math.sqrt(squaredSum / array.length);
    }

    /** Remove outliers within mConstStdevNumAverage standard deviations */
    private double averageWithoutOutliers(double[] array) throws Exception {
        double average = average(array);
        double stdev = stdev(array);
        double noOutlierCumSum = 0.0;
        int noOutlierCount = 0;
        for (int i = 0; i < array.length; i++) {
            if (Math.abs(average - array[i]) > mConstNumStdRemove * stdev) {
                continue;
            }
            noOutlierCumSum += array[i];
            noOutlierCount += 1;
        }
        // convert ns to ms
        return noOutlierCumSum / (noOutlierCount * 1000000);
    }

    /**
     * Returns the mask to be used by taskset when running RT-APP which will affine RTAPP to a
     * particular CPU, holding the RT-APP CPU variable constant in the environment from run-to-run
     */
    private String getAffineMaskForRTApp(String testCase) throws Exception {
        String parameterMaskValue = "";
        // for NEW CPU loads, we need to affine RT-App so UTIL_EST isn't affected by RT-App
        if (testCase.toUpperCase().startsWith("NEW_")) {
            for (Map.Entry<String, String> entry : sClusterAffineMap.entrySet()) {
                if (testCase.toUpperCase().contains(entry.getKey().toUpperCase())) {
                    parameterMaskValue = "taskset -a " + entry.getValue() + " ";
                    break;
                }
            }
        }
        return parameterMaskValue;
    }
}
