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

import static org.junit.Assert.*;

import com.android.tradefed.build.IBuildInfo;
import com.android.tradefed.testtype.DeviceTestCase;
import com.android.tradefed.testtype.IAbi;
import com.android.tradefed.testtype.IAbiReceiver;
import com.android.tradefed.testtype.IBuildReceiver;
import com.android.tradefed.log.LogUtil.CLog;

import org.apache.commons.compress.compressors.CompressorStreamFactory;

import org.junit.Before;
import org.junit.Test;

import java.io.BufferedOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;

public abstract class VirtTestCase extends DeviceTestCase implements IBuildReceiver, IAbiReceiver {

    private static final String JAR_RES_DIR = "/res";
    private static final String DEVICE_DIR = "/data/local/tmp/virt-test-hostside";

    private static final String KERNEL_DEVICE_PATH = DEVICE_DIR + "/kernel";
    private static final String RAMDISK_DEVICE_PATH = DEVICE_DIR + "/ramdisk";
    private static final String TEST_BINARY_DEVICE_PATH = DEVICE_DIR + "/test_binary";

    protected static final int EXIT_SUCCESS = 0;
    protected static final int EXIT_FAILURE = 1;

    private static final String EXEC_EXIT_CODE_PREFIX="ADB_EXEC_EXIT_CODE=";

    private static final int CID_RESERVED = 2;

    private IBuildInfo mBuildInfo;
    private IAbi mAbi;
    private String mAbiName;

    private String mKernelJarPath;
    private String mRamdiskJarPath;

    /**
     * Waits for device to be online, marks the most recent boottime of the device
     */
    @Before
    public void setUp() throws Exception {
        getDevice().waitForDeviceAvailable();
    }

    private boolean enableRoot() throws Exception {
        return getDevice().enableAdbRoot();
    }

    private boolean disableRoot() throws Exception {
        return getDevice().disableAdbRoot();
    }

    private ShellCommandResult runCommand(String cmd) throws Exception {
        String wrappedCmd = String.format("(%s); echo -n \"\\n%s$?\"", cmd, EXEC_EXIT_CODE_PREFIX);
        String output = getDevice().executeShellCommand(wrappedCmd);

        int lastNewLine = output.lastIndexOf('\n');
        if (lastNewLine == -1) {
            throw new IllegalStateException("Expected to find a new line in command output");
        }

        String lastLine = output.substring(lastNewLine + 1);
        if (!lastLine.startsWith(EXEC_EXIT_CODE_PREFIX)) {
            throw new IllegalArgumentException(String.format(
                    "Shell command did not print the exit code for '%s'", lastLine));
        }

        int exitCode;
        try {
            exitCode = Integer.parseInt(lastLine.substring(EXEC_EXIT_CODE_PREFIX.length()));
        } catch (NumberFormatException e) {
            throw new IllegalArgumentException(String.format(
                    "Could not get the exit code (%s) for '%s'", lastLine, cmd));
        }

        return new ShellCommandResult(output.substring(0, lastNewLine).trim(), exitCode);
    }

    private void extractResource(String resFilePath, File file) throws Exception {
        try (InputStream in = this.getClass().getResourceAsStream(resFilePath);
            OutputStream out = new BufferedOutputStream(new FileOutputStream(file))) {
            if (in == null) {
                throw new IllegalArgumentException("Resource not found: " + resFilePath);
            }
            byte[] buf = new byte[65536];
            int chunkSize;
            while ((chunkSize = in.read(buf)) != -1) {
                out.write(buf, 0, chunkSize);
            }
        }
    }

    private void pushResource(String resFilePath, String deviceFilePath) throws Exception {
        File resFile = File.createTempFile("VirtualizationHostTestResource", "");
        try {
            extractResource(resFilePath, resFile);
            getDevice().pushFile(resFile, deviceFilePath);
        } finally {
            resFile.delete();
        }
    }

    private boolean deleteResource(String deviceFilePath) throws Exception {
        return runCommand(String.format("rm %s", deviceFilePath)).exitCode == EXIT_SUCCESS;
    }

    private boolean makeExecutable(String deviceFilePath) throws Exception {
        return runCommand(String.format("chmod +x %s", deviceFilePath)).exitCode == EXIT_SUCCESS;
    }

    protected void prepareTest(String testBinaryPath) throws Exception {
        if (runCommand("ls /dev/kvm").exitCode != EXIT_SUCCESS) {
            throw new IllegalStateException(
                    "KVM device does not exist. Is it enabled in your kernel?");
        }

        if (runCommand("which crosvm").exitCode != EXIT_SUCCESS) {
            throw new IllegalStateException("CrosVM not found on device. Have you installed it?");
        }

        if (!enableRoot()) {
            throw new IllegalStateException("Cannot enable root on the target device");
        }

        pushResource(mKernelJarPath, KERNEL_DEVICE_PATH);
        pushResource(mRamdiskJarPath, RAMDISK_DEVICE_PATH);

        if (testBinaryPath != null) {
            String resPath = String.format("%s/%s", JAR_RES_DIR, testBinaryPath);
            pushResource(resPath, TEST_BINARY_DEVICE_PATH);

            if (!makeExecutable(TEST_BINARY_DEVICE_PATH)) {
                throw new IllegalStateException("Could not make test binary executable");
            }
        }
    }

    protected void finishTest() throws Exception {
        deleteResource(KERNEL_DEVICE_PATH);
        deleteResource(RAMDISK_DEVICE_PATH);
        deleteResource(TEST_BINARY_DEVICE_PATH);
        disableRoot();
    }

    protected ShellCommandResult runGuest(Integer cid, String... args) throws Exception {
        ArrayList<String> cmd = new ArrayList<>();
        ArrayList<String> params = new ArrayList<>();

        cmd.add("crosvm");
        cmd.add("run");

        cmd.add("--disable-sandbox");

        if (cid != null) {
            if (cid > CID_RESERVED) {
                cmd.add("--cid");
                cmd.add(cid.toString());
            } else {
                throw new IllegalArgumentException("Invalid CID " + cid);
            }
        }

        cmd.add("--initrd");
        cmd.add(RAMDISK_DEVICE_PATH);

        cmd.add("--params");
        params.add("--");
        for (String arg : args) {
            if (arg.indexOf(' ') != -1) {
                throw new IllegalArgumentException("TODO: implement quotes around arguments");
            } else if (arg.indexOf('"') != -1) {
                throw new IllegalArgumentException("TODO: implement escaping arguments");
            }
            params.add(arg);
        }
        // Surround command line args with quotes. Add space after the first
        // quote in case the first param starts with a '-' (interpreted as
        // a start of another command line option).
        cmd.add("\" " + String.join(" ", params) + " \"");

        cmd.add(KERNEL_DEVICE_PATH);

        return runCommand(String.join(" ", cmd));
    }

    protected ShellCommandResult runTestBinary(String... args) throws Exception {
        String cmd = String.format("%s %s", TEST_BINARY_DEVICE_PATH, String.join(" ", args));
        return runCommand(cmd);
    }

    @Override
    public void setBuild(IBuildInfo buildInfo) {
        // Get the build, this is used to access the APK.
        mBuildInfo = buildInfo;
    }

    @Override
    public void setAbi(IAbi abi) {
        mAbi = abi;
        if ("arm64-v8a".equals(mAbi.getName())) {
            mAbiName = "arm64";
        } else {
            throw new IllegalArgumentException("Unknown ABI: " + mAbi.getName());
        }
        updateAbiPaths();
    }

    private void updateAbiPaths() {
        mKernelJarPath = String.format("%s/kernel-%s", JAR_RES_DIR, mAbiName);
        mRamdiskJarPath = String.format("%s/ramdisk-%s", JAR_RES_DIR, mAbiName);
    }
}
