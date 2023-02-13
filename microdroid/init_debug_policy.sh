#!/vendor/bin/sh
# Copyright 2023 The Android Open Source Project
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

# Applies debug policies when booting microdroid

avf_debug_policy_root="/sys/firmware/devicetree/base/avf"

# If VM is debuggable or debug policy says so, send logs to outside ot the VM via the serial console.
# Otherwise logs are internally consumed at /dev/null
if [[ "$(getprop ro.boot.microdroid.debuggable)" == "1"
        || $(xxd -p "${avf_debug_policy_root}/guest/common/log") == "00000001" ]]; then
    setprop ro.log.file_logger.path /dev/hvc2
else
    setprop ro.log.file_logger.path /dev/null
fi

if [[ "$(getprop ro.boot.adb.enabled)" == "1"
        || $(xxd -p "${avf_debug_policy_root}/guest/microdroid/adb") == "00000001" ]]; then
    setprop microdroid.debug_policy.adbd 1
fi

if [[ "$(getprop ro.boot.microdroid.debuggable)" == "1"
        || $(xxd -p "${avf_debug_policy_root}/guest/microdroid/adb_root") == "00000001" ]]; then
    setprop ro.debuggable 1
else
    setprop ro.debuggable 0
fi
