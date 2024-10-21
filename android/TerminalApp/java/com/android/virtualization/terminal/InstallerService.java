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

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;
import android.util.Log;

import androidx.annotation.Nullable;

import com.android.internal.annotations.GuardedBy;
import com.android.virtualization.vmlauncher.InstallUtils;

import org.apache.commons.compress.archivers.ArchiveEntry;
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream;
import org.apache.commons.compress.compressors.gzip.GzipCompressorInputStream;

import java.io.BufferedInputStream;
import java.io.File;
import java.io.IOException;
import java.lang.ref.WeakReference;
import java.net.URL;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public class InstallerService extends Service {
    private static final String TAG = "InstallerService";

    private static final String NOTIFICATION_CHANNEL_ID = "installer";
    private static final int NOTIFICATION_ID = 1313; // any unique number among notifications

    // TODO(b/369740847): Replace this URL with dl.google.com
    private static final String IMAGE_URL =
            "https://github.com/ikicha/debian_ci/releases/download/first/images.tar.gz";

    private final Object mLock = new Object();

    private Notification mNotification;

    @GuardedBy("mLock")
    private boolean mIsInstalling;

    @GuardedBy("mLock")
    private List<IInstallProgressListener> mListeners = new ArrayList<>();

    private ExecutorService mExecutorService;

    @Override
    public void onCreate() {
        super.onCreate();

        // Create mandatory notification
        NotificationManager manager =
                (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
        if (manager.getNotificationChannel(NOTIFICATION_CHANNEL_ID) == null) {
            NotificationChannel channel =
                    new NotificationChannel(
                            NOTIFICATION_CHANNEL_ID,
                            getString(R.string.installer_notif_title_text),
                            NotificationManager.IMPORTANCE_DEFAULT);
            manager.createNotificationChannel(channel);
        }

        Intent intent = new Intent(this, MainActivity.class);
        PendingIntent pendingIntent =
                PendingIntent.getActivity(
                        this, /* requestCode= */ 0, intent, PendingIntent.FLAG_IMMUTABLE);
        mNotification =
                new Notification.Builder(this, NOTIFICATION_CHANNEL_ID)
                        .setContentTitle(getString(R.string.installer_notif_title_text))
                        .setContentText(getString(R.string.installer_notif_desc_text))
                        .setOngoing(true)
                        .setContentIntent(pendingIntent)
                        .build();

        mExecutorService = Executors.newSingleThreadExecutor();
    }

    @Nullable
    @Override
    public IBinder onBind(Intent intent) {
        return new InstallerServiceImpl(this);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        super.onStartCommand(intent, flags, startId);

        return START_STICKY;
    }

    @Override
    public void onDestroy() {
        super.onDestroy();

        if (mExecutorService != null) {
            mExecutorService.shutdown();
        }
    }

    private void requestInstall() {
        Log.i(TAG, "Installing..");

        // Make service to be long running, even after unbind.
        startService(new Intent(this, InstallerService.class));
        startForeground(
                NOTIFICATION_ID, mNotification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE);

        mExecutorService.execute(
                () -> {
                    // TODO(b/374015561): Provide progress update
                    boolean success = downloadFromSdcard() || downloadFromUrl();

                    stopForeground(STOP_FOREGROUND_REMOVE);

                    if (success) {
                        notifyCompleted();
                    }
                });
    }

    private boolean downloadFromSdcard() {
        // Installing from sdcard is preferred, but only supported only in debuggable build.
        if (Build.isDebuggable()) {
            Log.i(TAG, "trying to install /sdcard/linux/images.tar.gz");

            if (InstallUtils.installImageFromExternalStorage(this)) {
                Log.i(TAG, "image is installed from /sdcard/linux/images.tar.gz");
                return true;
            }
            Log.i(TAG, "There is no /sdcard/linux/images.tar.gz");
        } else {
            Log.i(TAG, "Non-debuggable build doesn't support installation from /sdcard/linux");
        }
        return false;
    }

    private boolean downloadFromUrl() {
        Log.i(TAG, "trying to download from " + IMAGE_URL);

        try (BufferedInputStream inputStream =
                        new BufferedInputStream(new URL(IMAGE_URL).openStream());
                TarArchiveInputStream tar =
                        new TarArchiveInputStream(new GzipCompressorInputStream(inputStream))) {
            ArchiveEntry entry;
            Path baseDir = new File(getFilesDir(), InstallUtils.PAYLOAD_DIR).toPath();
            Files.createDirectories(baseDir);
            while ((entry = tar.getNextEntry()) != null) {
                Path extractTo = baseDir.resolve(entry.getName());
                if (entry.isDirectory()) {
                    Files.createDirectories(extractTo);
                } else {
                    Files.copy(tar, extractTo, StandardCopyOption.REPLACE_EXISTING);
                }
            }
        } catch (IOException e) {
            Log.e(TAG, "Installation failed", e);
            notifyError(getString(R.string.installer_error_unknown));
            return false;
        }

        InstallUtils.resolvePathInVmConfig(this);
        return true;
    }

    private void notifyError(String displayText) {
        List<IInstallProgressListener> listeners;
        synchronized (mLock) {
            listeners = new ArrayList<>(mListeners);
            // We wouldn't install multiple times.. for now
            mListeners.clear();
        }

        for (IInstallProgressListener listener : listeners) {
            try {
                listener.onError(displayText);
            } catch (Exception e) {
                // ignore..
            }
        }
    }

    private void notifyCompleted() {
        List<IInstallProgressListener> listeners;
        synchronized (mLock) {
            listeners = new ArrayList<>(mListeners);
            // We wouldn't install multiple times.. for now
            mListeners.clear();
        }

        for (IInstallProgressListener listener : listeners) {
            try {
                listener.onCompleted();
            } catch (Exception e) {
                // ignore..
            }
        }
    }

    private static final class InstallerServiceImpl extends IInstallerService.Stub {
        // Holds weak reference to avoid Context leak
        private final WeakReference<InstallerService> mService;

        public InstallerServiceImpl(InstallerService service) {
            mService = new WeakReference<>(service);
        }

        private InstallerService ensureServiceConnected() throws RuntimeException {
            InstallerService service = mService.get();
            if (service == null) {
                throw new RuntimeException(
                        "Internal error: Installer service is being accessed after destroyed");
            }
            return service;
        }

        @Override
        public void requestInstall() {
            InstallerService service = ensureServiceConnected();
            synchronized (service.mLock) {
                service.requestInstall();
            }
        }

        @Override
        public void registerProgressListener(IInstallProgressListener listener) {
            InstallerService service = ensureServiceConnected();
            synchronized (service.mLock) {
                service.mListeners.add(listener);
            }
        }

        @Override
        public void unregisterProgressListener(IInstallProgressListener listener) {
            InstallerService service = ensureServiceConnected();
            synchronized (service.mLock) {
                service.mListeners.remove(listener);
            }
        }

        @Override
        public boolean isInstalling() {
            InstallerService service = ensureServiceConnected();
            synchronized (service.mLock) {
                return service.mIsInstalling;
            }
        }

        @Override
        public boolean isInstalled() {
            InstallerService service = ensureServiceConnected();
            synchronized (service.mLock) {
                return InstallUtils.isImageInstalled(service);
            }
        }
    }
}
