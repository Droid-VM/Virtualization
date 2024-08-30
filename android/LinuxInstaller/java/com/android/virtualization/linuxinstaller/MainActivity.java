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

package com.android.virtualization.linuxinstaller;

import android.annotation.WorkerThread;
import android.app.Activity;
import android.content.ComponentName;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.Bundle;
import android.os.Environment;
import android.os.SystemProperties;
import android.text.TextUtils;
import android.util.Log;
import android.view.WindowManager;
import android.widget.TextView;

import java.io.BufferedReader;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public class MainActivity extends Activity {
    private static final String TAG = "LinuxInstaller";
    private static final String ACTION_VM_TERMINAL = "android.virtualization.VM_TERMINAL";

    private static final Path DEST_DIR =
            Path.of(Environment.getExternalStorageDirectory().getPath(), "linux");

    private static final String ASSET_DIR = "linux";
    private static final Path VERSION_FILE = Path.of(DEST_DIR.toString(), "version");

    ExecutorService executorService = Executors.newSingleThreadExecutor();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);

        // Find VM Launcher
        ComponentName vmTerminalComponent = resolve(getPackageManager(), ACTION_VM_TERMINAL);
        if (vmTerminalComponent == null) {
            updateStatus("Failed to resolve VM terminal");
            return;
        }

        executorService.execute(
                () -> {
                    if (hasLocalAssets()) {
                        updateImageIfNeeded();
                        if (updateImageIfNeeded()) {
                            updateStatus("Enable terminal app...");
                            getPackageManager()
                                    .setComponentEnabledSetting(
                                            vmTerminalComponent,
                                            PackageManager.COMPONENT_ENABLED_STATE_ENABLED,
                                            PackageManager.DONT_KILL_APP);
                        }
                    }
                });
    }

    @WorkerThread
    private boolean hasLocalAssets() {
        try {
            String[] files = getAssets().list(ASSET_DIR);
            return files != null && files.length > 0;
        } catch (IOException e) {
            return false;
        }
    }

    @WorkerThread
    private boolean updateImageIfNeeded() {
        if (!isUpdateNeeded()) {
            Log.d(TAG, "No update needed.");
            return true;
        }

        try {
            if (Files.notExists(DEST_DIR)) {
                Files.createDirectory(DEST_DIR);
            }

            updateStatus("Copying images...");
            String[] files = getAssets().list(ASSET_DIR);
            for (String file : files) {
                updateStatus(file);
                Path dst = Path.of(DEST_DIR.toString(), file);
                updateFile(getAssets().open(ASSET_DIR + "/" + file), dst);
            }
        } catch (IOException e) {
            Log.e(TAG, "Error while updating image: " + e);
            updateStatus("Failed.");
            return false;
        }
        updateStatus("Done.");

        return extractImages(DEST_DIR.toAbsolutePath().toString());
    }

    @WorkerThread
    private boolean extractImages(String destDir) {
        updateStatus("Extracting images...");

        if (TextUtils.isEmpty(destDir)) {
            throw new RuntimeException("Internal error: destDir shouldn't be null");
        }

        SystemProperties.set("debug.custom_vm_setup.path", destDir);
        SystemProperties.set("debug.custom_vm_setup.done", "false");
        SystemProperties.set("debug.custom_vm_setup.start", "true");
        while (!SystemProperties.getBoolean("debug.custom_vm_setup.done", false)) {
            try {
                Thread.sleep(1000);
            } catch (Exception e) {
                Log.e(TAG, "Error while extracting image: " + e);
                updateStatus("Failed.");
                return false;
            }
        }

        updateStatus("Done.");
        return true;
    }

    @WorkerThread
    private boolean isUpdateNeeded() {
        Path[] pathsToCheck = {DEST_DIR, VERSION_FILE};
        for (Path p : pathsToCheck) {
            if (Files.notExists(p)) {
                Log.d(TAG, p.toString() + " does not exist.");
                return true;
            }
        }

        try {
            String installedVer = readLine(new FileInputStream(VERSION_FILE.toFile()));
            String updatedVer = readLine(getAssets().open(ASSET_DIR + "/version"));
            if (installedVer.equals(updatedVer)) {
                return false;
            }
            Log.d(TAG, "Version mismatch. Installed: " + installedVer + "  Updated: " + updatedVer);
        } catch (IOException e) {
            Log.e(TAG, "Error while checking version: " + e);
        }
        return true;
    }

    private static String readLine(InputStream input) throws IOException {
        try (BufferedReader reader = new BufferedReader(new InputStreamReader(input))) {
            return reader.readLine();
        } catch (IOException e) {
            throw e;
        }
    }

    private static void updateFile(InputStream input, Path path) throws IOException {
        try {
            Files.copy(input, path, StandardCopyOption.REPLACE_EXISTING);
        } finally {
            input.close();
        }
    }

    private void updateStatus(String line) {
        runOnUiThread(
                () -> {
                    TextView statusView = findViewById(R.id.status_txt_view);
                    statusView.append(line + "\n");
                });
    }

    private ComponentName resolve(PackageManager pm, String action) {
        Intent intent = new Intent(action);
        List<ResolveInfo> resolveInfos = pm.queryIntentActivities(intent, PackageManager.MATCH_ALL);
        if (resolveInfos.size() != 1) {
            Log.w(
                    TAG,
                    "Failed to resolve activity, action=" + action + ", resolved=" + resolveInfos);
            return null;
        }
        ActivityInfo activityInfo = resolveInfos.getFirst().activityInfo;
        // MainActivityAlias shows in Launcher
        return new ComponentName(activityInfo.packageName, activityInfo.name + "Alias");
    }
}
