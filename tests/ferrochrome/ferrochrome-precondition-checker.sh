#!/bin/bash

# Copyright 2024 Google Inc. All rights reserved.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

## Checks for preconditions for running ferrochrome

if [[ -n "${1}" ]]; then
  ADB="adb -s ${1}"
else
  ADB="adb"
fi

if [[ "$(eval ${ADB} root)" == *"cannot"* ]]; then
  >&2 echo "Failed to run adb root"
  exit 1
fi

if [[ -z "$(eval ${ADB} shell pm list packages vmlauncher)" ]]; then
  >&2 echo "Failed to find vmlauncher"
  exit 1
fi

free_space=$(eval ${ADB} shell df /data/local | tail -1 | awk '{print $4}')
if [[ ${free_space} -gt 7340032 ]]; then
  >&2 echo "Insufficient space. Need at least 7G, but was ${free_space}"
  exit 1
fi

cpu_abi=$(eval ${ADB} shell getprop ro.product.cpu.abi)
if [[ "${cpu_abi}" != "arm64"* ]]; then
  >&2 echo "Unsupported architecture. Requires arm64, but was ${cpu_abi}"
  exit 1
fi

device=$(eval ${ADB} shell getprop ro.product.vendor.device)
if [[ "${device}" == "vsock_"* ]]; then
  >&2 echo "Unsupported device. Cuttlefish isn't supported"
  exit 1
fi
