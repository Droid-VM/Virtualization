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

package com.android.virtualization.terminal;

import android.Manifest;
import android.annotation.MainThread;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.ServiceConnection;
import android.content.pm.PackageManager;
import android.os.Bundle;
import android.os.IBinder;

import androidx.appcompat.app.AppCompatActivity;

import java.lang.ref.WeakReference;

public abstract class BaseActivity extends AppCompatActivity {
    private static final int POST_NOTIFICATIONS_PERMISSION_REQUEST_CODE = 101;

    private IInstallerService mInstallerService;
    private ServiceConnection mInstallerServiceConnection;

    @Override
    public void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        Intent intent = new Intent(this, InstallerService.class);
        mInstallerServiceConnection = new InstallerServiceConnection(this);
        if (!bindService(intent, mInstallerServiceConnection, Context.BIND_AUTO_CREATE)) {
            handleCriticalError(new Exception("Failed to connect to installer service"));
        }
    }

    @Override
    public void onResume() {
        super.onResume();

        if (getApplicationContext().checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(
                    new String[] {Manifest.permission.POST_NOTIFICATIONS},
                    POST_NOTIFICATIONS_PERMISSION_REQUEST_CODE);
        }
    }

    @Override
    public void onDestroy() {
        super.onDestroy();

        if (mInstallerServiceConnection != null) {
            unbindService(mInstallerServiceConnection);
            mInstallerServiceConnection = null;
        }
    }

    @MainThread
    public IInstallerService getInstallerService() {
        return mInstallerService;
    }

    // Handle critical error that shouldn't happen. Only called when assertion fails.
    @MainThread
    public abstract void handleCriticalError(Exception e);

    @MainThread
    public abstract void handleInstallerServiceConnected();

    @MainThread
    public abstract void handleInstallerServiceDisconnected();

    @MainThread
    public static final class InstallerServiceConnection implements ServiceConnection {
        private final WeakReference<BaseActivity> mActivity;

        InstallerServiceConnection(BaseActivity activity) {
            mActivity = new WeakReference<>(activity);
        }

        @Override
        public void onServiceConnected(ComponentName name, IBinder service) {
            BaseActivity activity = mActivity.get();
            if (activity == null || activity.mInstallerServiceConnection == null) {
                // Ignore incoming connection or disconnection after activity is destroyed.
                return;
            }
            if (service == null) {
                activity.handleCriticalError(new Exception("service shouldn't be null"));
            }

            activity.mInstallerService = IInstallerService.Stub.asInterface(service);
            activity.handleInstallerServiceConnected();
        }

        @Override
        public void onServiceDisconnected(ComponentName name) {
            BaseActivity activity = mActivity.get();
            if (activity == null || activity.mInstallerServiceConnection == null) {
                // Ignore incoming connection or disconnection after activity is destroyed.
                return;
            }

            if (activity.mInstallerServiceConnection != null) {
                activity.unbindService(activity.mInstallerServiceConnection);
                activity.mInstallerServiceConnection = null;
            }
            activity.handleInstallerServiceDisconnected();
        }
    }
}
