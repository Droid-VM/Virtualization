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

import static android.net.NetworkCapabilities.NET_CAPABILITY_INTERNET;
import static android.net.NetworkCapabilities.NET_CAPABILITY_NOT_METERED;

import android.annotation.MainThread;
import android.net.ConnectivityManager;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.os.Bundle;
import android.os.FileUtils;
import android.os.RemoteException;
import android.text.format.Formatter;
import android.util.Log;
import android.widget.CheckBox;
import android.widget.TextView;
import android.widget.Toast;

import java.lang.ref.WeakReference;
import java.util.concurrent.ExecutorService;

public class InstallerActivity extends BaseActivity {
    private static final String TAG = "LinuxInstaller";

    private static final long ESTIMATED_IMG_SIZE_BYTES = FileUtils.parseSize("350MB");

    private ExecutorService mExecutorService;
    private CheckBox mAllowMeteredCheckBox;
    private TextView mInstallButton;

    private InstallProgressListener mInstallProgressListener;
    private boolean mInstallRequested;

    @Override
    public void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        mInstallProgressListener = new InstallProgressListener(this);

        setContentView(R.layout.activity_installer);

        TextView desc = (TextView) findViewById(R.id.installer_desc);
        desc.setText(
                getString(
                        R.string.installer_desc_text_format,
                        Formatter.formatShortFileSize(this, ESTIMATED_IMG_SIZE_BYTES)));

        mAllowMeteredCheckBox = (CheckBox) findViewById(R.id.installer_allow_metered_checkbox);
        mInstallButton = (TextView) findViewById(R.id.installer_install_button);

        mInstallButton.setOnClickListener(
                (event) -> {
                    requestInstall();
                });
    }

    @Override
    public void onDestroy() {
        IInstallerService service = getInstallerService();
        if (service != null) {
            try {
                service.unregisterProgressListener(mInstallProgressListener);
            } catch (RemoteException e) {
                // ignore any error while destroying.
            }
        }

        super.onDestroy();
    }

    @Override
    public void handleCriticalError(Exception e) {
        Toast.makeText(
                        this,
                        e.getMessage() + ". File a bugreport to go/ferrochrome-bug",
                        Toast.LENGTH_LONG)
                .show();
        Log.e(TAG, "Internal error", e);
        finish();
    }

    private void setInstallEnabled(boolean enable) {
        mInstallButton.setEnabled(enable);
        mAllowMeteredCheckBox.setEnabled(enable);

        int resId =
                enable
                        ? R.string.installer_install_button_enabled_text
                        : R.string.installer_install_button_disabled_text;
        mInstallButton.setText(getString(resId));
    }

    private boolean checkNetworkCapabilities() {
        ConnectivityManager manager =
                (ConnectivityManager) getSystemService(ConnectivityManager.class);

        Network network = manager.getActiveNetwork();
        if (network == null) {
            return false;
        }
        NetworkCapabilities capability = manager.getNetworkCapabilities(network);
        if (!capability.hasCapability(NET_CAPABILITY_INTERNET)) {
            return false;
        }
        if (!mAllowMeteredCheckBox.isChecked()
                && capability.hasCapability(NET_CAPABILITY_NOT_METERED)) {
            return false;
        }
        return true;
    }

    @MainThread
    private void requestInstall() {
        setInstallEnabled(/* enable= */ false);

        IInstallerService service = getInstallerService();
        if (service != null) {
            try {
                if (checkNetworkCapabilities()) {
                    service.requestInstall();
                } else {
                    handleError(getString(R.string.installer_error_network));
                }
            } catch (RemoteException e) {
                handleCriticalError(e);
            }
        } else {
            Log.d(TAG, "requestInstall() is called, but not yet connected");
            mInstallRequested = true;
        }
    }

    @MainThread
    @Override
    public void handleInstallerServiceConnected() {
        IInstallerService service = getInstallerService();
        try {
            if (service.isInstalled()) {
                // Finishing this activity will trigger MainActivity::onResume(),
                // and VM will be started from there.
                finish();
            } else {
                service.registerProgressListener(mInstallProgressListener);
            }

            if (mInstallRequested) {
                requestInstall();
            } else if (service.isInstalling()) {
                setInstallEnabled(false);
            }
        } catch (RemoteException e) {
            handleCriticalError(e);
        }
    }

    @MainThread
    @Override
    public void handleInstallerServiceDisconnected() {
        handleCriticalError(new Exception("InstallerService is destroyed while in use"));
    }

    @MainThread
    private void handleError(String displayText) {
        Toast.makeText(this, displayText, Toast.LENGTH_LONG).show();
        setInstallEnabled(true);
    }

    private static class InstallProgressListener extends IInstallProgressListener.Stub {
        private final WeakReference<InstallerActivity> mActivity;

        InstallProgressListener(InstallerActivity activity) {
            mActivity = new WeakReference<>(activity);
        }

        @Override
        public void onCompleted() {
            InstallerActivity activity = mActivity.get();
            if (activity == null) {
                // Ignore incoming connection or disconnection after activity is destroyed.
                return;
            }

            // MainActivity will be resume and handle rest of progress.
            activity.finish();
        }

        @Override
        public void onError(String displayText) {
            InstallerActivity context = mActivity.get();
            if (context == null) {
                // Ignore incoming connection or disconnection after activity is destroyed.
                return;
            }

            context.runOnUiThread(
                    () -> {
                        InstallerActivity activity = mActivity.get();
                        if (activity == null) {
                            // Ignore incoming connection or disconnection after activity is
                            // destroyed.
                            return;
                        }

                        // MainActivity will be resume and handle rest of progress.
                        activity.handleError(displayText);
                    });
        }
    }
}
