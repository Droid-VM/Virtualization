#!/usr/bin/env python3
#
# Copyright (C) 2025 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#      http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#

import logging
import os
import pkgutil
import shutil
import subprocess
import tempfile
import unittest

def _RunCommand(cmd, timeout):
    kill = lambda process:process.kill()
    proc = subprocess.Popen(args=cmd,
                            stderr=subprocess.PIPE,
                            stdout=subprocess.PIPE,
                            universal_newlines=True)
    try:
        out, err = proc.communicate(timeout=timeout)
    except TimeoutExpired:
        proc.kill()
        out, err = proc.communicate()

    return out, err, proc.returncode

class AvfEarlyVmTest(unittest.TestCase):
    _DEFAULT_ADB_TIMEOUT = 300

    def _AdbTryExecute(self, cmd_list, timeout=_DEFAULT_ADB_TIMEOUT):
        cmd = ["adb", "-s", self._serial_number]
        cmd.extend(cmd_list)
        return _RunCommand(cmd, timeout)

    def _AdbExecute(self, cmd_list, timeout=_DEFAULT_ADB_TIMEOUT):
        out, err, returncode = self._AdbTryExecute(cmd_list, timeout)
        self.assertEqual(returncode, 0, f"adb {cmd_list} failed: {err}")
        return out

    def _AdbShell(self, cmd):
        return self._AdbExecute(["shell"] + cmd)

    def _AdbEnableVerity(self):
        self._AdbTryExecute(["enable-verity", "-R"])
        self._AdbTryExecute(["wait-for-device"])

    def _AdbRemount(self):
        self._AdbTryExecute(["disable-verity", "-R"])
        self._AdbExecute(["wait-for-device"])
        self._AdbExecute(["remount", "-R"])

    def _AdbRoot(self):
        self._AdbExecute(["root"])
        self._AdbExecute(["wait-for-device"])

    def _AdbPush(self, path, dest):
        self._AdbExecute(["push", path, dest])

    def _AdbGetProp(self, prop):
        return self._AdbShell(["getprop", prop]).strip()

    def _GetDataFile(self, name, dest):
        blob = pkgutil.get_data("avf_early_vm_test", name)
        self.assertTrue(blob, f"{name} doesn't exist. Is this binary corrupted?")
        with open(dest, "wb") as f:
            f.write(blob)

    def setUp(self):
        self._serial_number = os.environ.get("ANDROID_SERIAL")
        self.assertTrue(self._serial_number, "$ANDROID_SERIAL is empty.")
        self._temp_dir = tempfile.mkdtemp()
        self._launcher_path = os.path.join(self._temp_dir, "avf_early_vm_test_launcher")
        self._rialto_path = os.path.join(self._temp_dir, "rialto.bin")
        self._early_vms_xml_path = os.path.join(self._temp_dir, "early_vms_rialto_test.xml")
        self._GetDataFile("avf_early_vm_test_launcher", self._launcher_path)
        self._GetDataFile("rialto.bin", self._rialto_path)
        self._GetDataFile("early_vms_rialto_test.xml", self._early_vms_xml_path)

    def tearDown(self):
        shutil.rmtree(self._temp_dir)
        self._AdbEnableVerity()

    def _IsNonProtectedVmSupported(self):
        prop = self._AdbGetProp("ro.boot.hypervisor.vm.supported")
        return prop == "1" or prop == "true"

    def _IsProtectedVmSupported(self):
        prop = self._AdbGetProp("ro.boot.hypervisor.protected_vm.supported")
        return prop == "1" or prop == "true"

    def _TestAvfEarlyVm(self, protected):
        self._AdbRemount()
        self._AdbRoot()
        self._AdbPush(self._launcher_path, "/system_ext/bin/avf_early_vm_test_launcher")
        self._AdbShell(["mkdir", "-p", "/system_ext/etc/avf"])
        self._AdbPush(self._rialto_path, "/system_ext/etc/avf/rialto_test.bin")
        self._AdbPush(self._early_vms_xml_path, "/system_ext/etc/avf/early_vms_rialto_test.xml")

        launcher_cmd = ["/system_ext/bin/avf_early_vm_test_launcher", "--kernel", "/system_ext/etc/avf/rialto_test.bin"]
        if protected:
            launcher_cmd.append("--protected")

        self._AdbShell(launcher_cmd)

    def testAvfEarlyVmNonProtected(self):
        self._AdbExecute(["wait-for-device"])
        if "arm64" not in self._AdbGetProp("ro.product.cpu.abilist"):
            logging.info("Skip test for a device not supporting arm64")
            return
        if not self._IsNonProtectedVmSupported():
            logging.info("Skip test where non-protected VMs are not supported")
            return
        self._TestAvfEarlyVm(False)

    def testAvfEarlyVmProtected(self):
        self._AdbExecute(["wait-for-device"])
        if "arm64" not in self._AdbGetProp("ro.product.cpu.abilist"):
            logging.info("Skip test for a device not supporting arm64")
            return
        if not self._IsProtectedVmSupported():
            logging.info("Skip test where non-protected VMs are not supported")
            return
        self._TestAvfEarlyVm(True)


if __name__ == '__main__':
    # Setting verbosity is required to generate output that the TradeFed test
    # runner can parse.
    unittest.main(verbosity=3)
