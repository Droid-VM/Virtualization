/*
 * Copyright (C) 2024 The Android Open Source Project
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

package com.android.virtualization.ferrochrome;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;
import android.util.Log;

public class OpenUrlActivity extends Activity {
    private static final String TAG = OpenUrlActivity.class.getSimpleName();

    private static final String ACTION_VM_LAUNCHER = "android.virtualization.VM_VIEW";

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        boolean isRoot = isTaskRoot();
        finish();
        if (isRoot) {
            Log.w(
                    TAG,
                    "Cannot open URL without starting "
                            + FerrochromeActivity.class.getSimpleName()
                            + " first, starting it now");
            startActivity(
                    new Intent(this, FerrochromeActivity.class).setAction(Intent.ACTION_MAIN));
            return;
        }
        // View the text payload in VM.
        startActivity(
                new Intent("android.virtualization.VM_VIEW")
                        .setFlags(
                                Intent.FLAG_ACTIVITY_SINGLE_TOP
                                        | Intent.FLAG_ACTIVITY_PREVIOUS_IS_TOP)
                        .putExtras(getIntent()));
    }
}
