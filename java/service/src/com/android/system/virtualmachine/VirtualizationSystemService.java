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

package com.android.system.virtualmachine;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.os.IBinder;
import android.os.ServiceManager;
import android.system.virtualizationmaintenance.IVirtualizationMaintenance;
import android.util.Slog;
import com.android.server.SystemService;


public class VirtualizationSystemService extends SystemService {
    private static final String TAG = "VirtualizationSystemService";

    private static final String VSI_SERVICE_NAME = "android.system.virtualizationmaintenance";

    private BroadcastReceiver mBroadcastReceiver;

    public VirtualizationSystemService(Context context) {
        super(context);
        Slog.w(TAG, "@@@@ VSS created");
    }

    @Override
    public void onStart() {
        Slog.w(TAG, "@@@@ VSS.onStart()");

        mBroadcastReceiver =
                new BroadcastReceiver() {
                    @Override
                    public void onReceive(Context context, Intent intent) {
                        int userId =
                                intent.getIntExtra(
                                        Intent.EXTRA_USER_HANDLE, /* @@@ UserHandle.USER_NULL */
                                        -10000);
                        Slog.w(
                                TAG,
                                "@@@@ onReceive(context="
                                        + context
                                        + ", intent="
                                        + intent
                                        + ") userId="
                                        + userId);
                        String packageName;
                        switch (intent.getAction()) {
                            case Intent.ACTION_PACKAGE_REMOVED:
                                packageName = packageNameFromIntent(intent);
                                Slog.w(
                                        TAG,
                                        "@@@@ package '"
                                                + packageName
                                                + "' removed for user "
                                                + userId);
                                break;
                            case Intent.ACTION_PACKAGE_CHANGED:
                                packageName = packageNameFromIntent(intent);
                                Slog.w(
                                        TAG,
                                        "@@@@ package '"
                                                + packageName
                                                + "' changed for user "
                                                + userId);
                                break;
                            case Intent.ACTION_USER_REMOVED:
                                Slog.w(TAG, "@@@@ user " + userId + " removed");
                                break;
                        }

                        Slog.w(TAG, "@@@@ talk to " + VSI_SERVICE_NAME);
                        IVirtualizationMaintenance service = getMaintenanceService();
                        Slog.w(TAG, "@@@@ talk to " + service);
                    }
                };
        IntentFilter packageFilter = new IntentFilter();
        // ?? "protected intent that can only be sent by the system"
        packageFilter.addAction(Intent.ACTION_PACKAGE_REMOVED);
        packageFilter.addAction(Intent.ACTION_PACKAGE_CHANGED);
        packageFilter.addAction(Intent.ACTION_PACKAGE_FULLY_REMOVED);
        packageFilter.addAction(Intent.ACTION_PACKAGE_FIRST_LAUNCH);

        Context context = getContext();

        context.registerReceiverForAllUsers(
                mBroadcastReceiver,
                packageFilter,
                null /* broadcast permission */,
                null /* handler */);

        IntentFilter userFilter = new IntentFilter();
        userFilter.addAction(
                Intent.ACTION_USER_REMOVED); // Need android.Manifest.permission.MANAGE_USERS ?
        userFilter.addAction(Intent.ACTION_USER_ADDED);
        userFilter.addAction(Intent.ACTION_USER_UNLOCKED);

        context.registerReceiverForAllUsers(
                mBroadcastReceiver,
                userFilter,
                null /* broadcast permission */,
                null /* handler */);
        Slog.w(TAG, "@@@@ VSS.onStart() registered broadcast receivers");
    }

    private static String packageNameFromIntent(Intent intent) {
        return intent.getDataString().substring("package:".length());
    }

    private static synchronized IVirtualizationMaintenance getMaintenanceService() {
        final IBinder service = ServiceManager.waitForService(VSI_SERVICE_NAME);
        if (service == null) {
            Slog.e(TAG, "Unable to acquire IVirtualizationMaintenance");
            return null;
        }
        Slog.e(TAG, "@@@@ got an IBinder object: " + service);
        return IVirtualizationMaintenance.Stub.asInterface(service);
    }
}
