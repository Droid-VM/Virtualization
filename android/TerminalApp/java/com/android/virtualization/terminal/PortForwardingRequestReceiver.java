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

package com.android.virtualization.terminal;

import android.app.NotificationManager;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;

public final class PortForwardingRequestReceiver extends BroadcastReceiver {
    public static final String ACTION_PORT_FORWARDING = "android.virtualization.PORT_FORWARDING";

    @Override
    public void onReceive(Context context, Intent intent) {
        String action = intent.getAction();
        switch (action) {
            case ACTION_PORT_FORWARDING:
                performActionPortForwarding(context, intent);
                break;
        }
    }

    private void performActionPortForwarding(Context context, Intent intent) {
        int port = intent.getIntExtra("port", 0);
        boolean enabled = intent.getBooleanExtra("enabled", false);

        SharedPreferences sharedPref =
                context.getSharedPreferences(
                        context.getResources().getString(R.string.preference_file_key),
                        Context.MODE_PRIVATE);
        SharedPreferences.Editor editor = sharedPref.edit();
        editor.putBoolean(
                context.getString(R.string.preference_forwarding_port_is_enabled)
                        + Integer.toString(port),
                enabled);
        editor.apply();

        context.getSystemService(NotificationManager.class).cancel(port);
    }
}
