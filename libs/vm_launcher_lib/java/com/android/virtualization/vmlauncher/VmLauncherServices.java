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

package com.android.virtualization.vmlauncher;

import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.Parcel;
import android.os.ResultReceiver;

import java.util.List;

public class VmLauncherServices {
    private static String getVmLauncherServicePackageName(Context context) {
        Intent intent = new Intent(ACTION_START_VM_LAUNCHER_SERVICE);
        PackageManager pm = context.getPackageManager();
        List<ResolveInfo> resolveInfos =
                pm.queryIntentServices(intent, PackageManager.MATCH_DEFAULT_ONLY);
        if (resolveInfos == null || resolveInfos.size() != 1) {
            return null;
        }
        return resolveInfos.get(0).serviceInfo.packageName;
    }

    public static void startVmLauncherService(Context context, VmLauncherServiceCallback callback) {
        Intent i = new Intent();
        i.putExtra(
                Intent.EXTRA_RESULT_RECEIVER,
                getResultReceiverForIntent(
                        new ResultReceiver(new Handler(Looper.myLooper())) {
                            @Override
                            protected void onReceiveResult(int resultCode, Bundle resultData) {
                                if (callback == null) {
                                    return;
                                }
                                switch (resultCode) {
                                    case RESULT_START:
                                        callback.onVmStart(resultData.getString(KEY_VM_NAME));
                                        return;
                                    case RESULT_STOP:
                                        callback.onVmStop();
                                        return;
                                    case RESULT_ERROR:
                                        callback.onVmError();
                                        return;
                                    case RESULT_IPADDR:
                                        callback.onIpAddrAvailable(
                                                resultData.getString(KEY_VM_IP_ADDR));
                                        return;
                                }
                            }
                        }));
        i.setAction(ACTION_START_VM_LAUNCHER_SERVICE);
        i.setPackage(getVmLauncherServicePackageName(context));
        context.startForegroundService(i);
    }

    public static final String ACTION_START_VM_LAUNCHER_SERVICE =
            "android.virtualization.START_VM_LAUNCHER_SERVICE";

    public static final int RESULT_START = 0;
    public static final int RESULT_STOP = 1;
    public static final int RESULT_ERROR = 2;
    public static final int RESULT_IPADDR = 3;

    public static final String KEY_VM_NAME = "name";
    public static final String KEY_VM_IP_ADDR = "ip_addr";

    public interface VmLauncherServiceCallback {
        void onVmStart(String vmName);

        void onVmStop();

        void onVmError();

        void onIpAddrAvailable(String ipAddr);
    }

    private static ResultReceiver getResultReceiverForIntent(ResultReceiver r) {
        Parcel parcel = Parcel.obtain();
        r.writeToParcel(parcel, 0);
        parcel.setDataPosition(0);
        r = ResultReceiver.CREATOR.createFromParcel(parcel);
        parcel.recycle();
        return r;
    }
}
