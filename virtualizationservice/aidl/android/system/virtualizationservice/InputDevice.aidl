/*
 * Copyright 2024 The Android Open Source Project
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
package android.system.virtualizationservice;

union InputDevice {
    parcelable Keyboard {
        ParcelFileDescriptor pfd;
    }

    parcelable SingleTouch {
        ParcelFileDescriptor pfd;
        // Default values come from https://crosvm.dev/book/devices/input.html#single-touch
        int width = 1280;
        int height = 1080;
        @utf8InCpp String name;
    }

    parcelable Evdev {
        ParcelFileDescriptor pfd;
    }

    Keyboard keyboard;
    SingleTouch singleTouch;
    Evdev evdev;
}
