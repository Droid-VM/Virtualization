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
import android.content.ComponentName;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.os.Bundle;
import android.os.SystemProperties;
import android.util.Log;
import android.widget.TextView;

import java.io.IOException;
import java.io.InputStream;
import java.net.URL;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

public class FerrochromeActivity extends Activity {
    ExecutorService executorService = Executors.newSingleThreadExecutor();
    private static final String TAG = "FerrochromeActivity";

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_ferrochrome);
        findViewById(R.id.launch_btn)
                .setOnClickListener(
                        (v) -> {
                            startActivity(
                                    new Intent()
                                            .setClassName(
                                                    getVmLauncherAppPackageName(),
                                                    "com.android.virtualization.vmlauncher.MainActivity"));
                        });
        executorService.execute(
                () -> {
                    if (Files.notExists(Path.of("/sdcard/chromiumos_test_image.bin"))
                            || Files.notExists(Path.of("/sdcard/vmlinuz"))) {

                        updateStatus("image doesn't exist");
                        updateStatus("download image");
                        if (download("R127-15916.0.0")) {
                            updateStatus("download done");
                        } else {
                            updateStatus("download failed, check internet connection and retry");
                            return;
                        }
                    } else {
                        updateStatus("there are already downloaded images");
                    }
                    updateStatus("write down vm config");
                    copyVmConfigJson();
                    updateStatus("custom_vm_setup: copy files to /data/local/tmp");
                    SystemProperties.set("debug.custom_vm_setup.start", "true");
                    while (SystemProperties.getBoolean("debug.custom_vm_setup.start", false)
                            == false) {
                        // Wait for custom_vm_setup
                        try {
                            Thread.sleep(500);
                        } catch (Exception e) {
                            Log.d(TAG, e.toString());
                        }
                        updateStatus("wait for custom_vm_setup");
                    }
                    updateStatus("enable vmlauncher");
                    enableVmLauncher();
                    updateStatus("ready for ferrochrome");
                    runOnUiThread(() -> findViewById(R.id.launch_btn).setEnabled(true));
                });
    }

    private String getVmLauncherAppPackageName() {
        PackageManager pm = getPackageManager();
        for (String packageName :
                new String[] {
                    "com.android.virtualization.vmlauncher",
                    "com.google.android.virtualization.vmlauncher"
                }) {
            try {
                pm.getPackageInfo(packageName, 0);
                return packageName;
            } catch (PackageManager.NameNotFoundException e) {
                continue;
            }
        }
        return null;
    }

    private void enableVmLauncher() {
        PackageManager pm = getPackageManager();
        pm.setComponentEnabledSetting(
                new ComponentName(
                        getVmLauncherAppPackageName(),
                        "com.android.virtualization.vmlauncher.MainActivity"),
                PackageManager.COMPONENT_ENABLED_STATE_ENABLED,
                PackageManager.DONT_KILL_APP);
        return;
    }

    private void updateStatus(String line) {
        Log.d(TAG, line);
        runOnUiThread(
                () -> {
                    TextView statusView = findViewById(R.id.status_txt_view);
                    statusView.append(line + "\n");
                });
    }

    private void copyVmConfigJson() {
        try (InputStream is = getResources().openRawResource(R.raw.vm_config)) {
            Files.copy(is, Path.of("/sdcard/vm_config.json"), StandardCopyOption.REPLACE_EXISTING);
        } catch (IOException e) {
            updateStatus(e.toString());
        }
    }

    private boolean download(String version) {
        String urlString =
                "https://storage.googleapis.com/chromiumos-image-archive/ferrochrome-public/"
                        + version
                        + "/image.zip";
        boolean hasKernel = false;
        boolean hasImage = false;
        try (InputStream is = (new URL(urlString)).openStream();
                ZipInputStream zis = new ZipInputStream(is)) {
            ZipEntry entry;
            while ((entry = zis.getNextEntry()) != null) {
                Path dest;
                if (entry.getName().contains("chromiumos_test_image.bin")) {
                    dest = Path.of("/sdcard/chromiumos_test_image.bin");
                    hasImage = true;
                } else if (entry.getName().contains("boot_images/vmlinuz-")) {
                    dest = Path.of("/sdcard/vmlinuz");
                    hasKernel = true;
                } else {
                    continue;
                }
                updateStatus("copy " + entry.getName() + " start");
                Files.copy(zis, dest, StandardCopyOption.REPLACE_EXISTING);
                updateStatus("copy " + entry.getName() + " done");
                if (hasImage && hasKernel) {
                    break;
                }
            }
        } catch (Exception e) {
            updateStatus(e.toString());
            return false;
        }
        return true;
    }
}
