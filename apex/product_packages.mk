#
# Copyright (C) 2021 Google Inc.
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

# To include the APEX in your build, insert this in your device.mk:
#   $(call inherit-product, $(SRC_TARGET_DIR)/product/isolated_compilation.mk)

# TODO(b/205977754): Inline this file at all places it is used and remove it

$(call inherit-product, $(SRC_TARGET_DIR)/product/isolated_compilation.mk)
