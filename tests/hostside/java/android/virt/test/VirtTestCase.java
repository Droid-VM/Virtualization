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

import org.junit.After;
import org.junit.Before;

import java.io.BufferedOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;

public abstract class VirtTestCase extends DeviceTestCase implements IAbiReceiver {

    private static final String JAR_RES_DIR = "/res";
    private static final String DEVICE_DIR = "/data/local/tmp/virt-test";

    private static final String KERNEL_DEVICE_PATH = DEVICE_DIR + "/kernel";
    private static final String RAMDISK_DEVICE_PATH = DEVICE_DIR + "/ramdisk";

    private static final int    CID_RESERVED = 2;

    private IAbi mAbi;

    @Before
    public void setUp() throws Exception {
        String kernelResPath = String.format("%s/kernel-%s", JAR_RES_DIR, getAbiName());
        String ramdiskResPath = String.format("%s/ramdisk-%s", JAR_RES_DIR, getAbiName());

        getDevice().waitForDeviceAvailable();
        pushResource(kernelResPath, KERNEL_DEVICE_PATH);
        pushResource(ramdiskResPath, RAMDISK_DEVICE_PATH);
    }

    @After
    public void tearDown() throws Exception {
        getDevice().deleteFile(KERNEL_DEVICE_PATH);
        getDevice().deleteFile(RAMDISK_DEVICE_PATH);
    }

    private String getAbiName() {
        String name = mAbi.getName();
        if ("arm64-v8a".equals(name)) {
            name = "arm64";
        }
        return name;
    }

    private static void extractResource(String resFilePath, File file) throws Exception {
        try (InputStream in = VirtTestCase.class.getResourceAsStream(resFilePath);
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

    protected String getDevicePathForTestBinary(String targetName) throws Exception {
        String path = String.format("%s/%s/%s", DEVICE_DIR, getAbiName(), targetName);
        if (!getDevice().doesFileExist(path)) {
            throw new IllegalArgumentException(String.format(
                    "Binary for target %s not found on device at \"%s\"", targetName, path));
        }
        return path;
    }

    protected static String createCommand(String prog, Object... args) {
        ArrayList<String> strings = new ArrayList<>();
        strings.add(prog);
        for (Object arg : args) {
            strings.add(arg.toString());
        }
        for (String str : strings) {
            if (str.indexOf(' ') != -1) {
                throw new IllegalArgumentException("TODO: implement quotes around arguments");
            } else if (str.indexOf('\'') != -1) {
                throw new IllegalArgumentException("TODO: implement escaping arguments");
            }
        }
        return String.join(" ", strings);
    }

    protected static String getVmCommand(String guestCmd, Integer cid) {
        ArrayList<String> cmd = new ArrayList<>();

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
        cmd.add(String.format("'%s'", guestCmd));

        cmd.add(KERNEL_DEVICE_PATH);

        return String.join(" ", cmd);
    }

    @Override
    public void setAbi(IAbi abi) {
        mAbi = abi;
    }
}
