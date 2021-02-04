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

import com.android.tradefed.testtype.DeviceTestCase;

import org.junit.Before;

import java.util.ArrayList;

public abstract class VirtTestCase extends DeviceTestCase {

    private static final String DEVICE_DIR = "/data/local/tmp/virt-test";

    private static final int CID_RESERVED = 2;


    @Before
    public void setUp() throws Exception {
        getDevice().waitForDeviceAvailable();
    }

    protected String getDevicePathForTestBinary(String targetName) throws Exception {
        String path = String.format("%s/%s", DEVICE_DIR, targetName);
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

    protected String getVmCommand(String guestCmd, Integer cid) throws Exception {
        ArrayList<String> cmd = new ArrayList<>();

        cmd.add("/apex/com.android.virt/bin/crosvm");
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
        cmd.add(getDevicePathForTestBinary("initramfs"));

        // Soong doesn't support installing a file directly under the root directory. Therefore the
        // init program is always installed as /bin/init. But kernel demands that the initrd has
        // /init in it [1]. Override this using the `rdinit` param.
        // [1] https://github.com/torvalds/linux/blob/master/Documentation/driver-api/early-userspace/early_userspace_support.rst#how-does-it-work
        cmd.add("--params 'rdinit=/bin/init'");

        cmd.add("--params");
        cmd.add(String.format("'%s'", guestCmd));

        cmd.add(getDevicePathForTestBinary("kernel"));

        return String.join(" ", cmd);
    }
}
