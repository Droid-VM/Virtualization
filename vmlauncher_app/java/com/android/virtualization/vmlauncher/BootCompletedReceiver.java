package com.android.virtualization.vmlauncher;

import android.content.BroadcastReceiver;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;

import java.nio.file.Files;
import java.nio.file.Path;

public class BootCompletedReceiver extends BroadcastReceiver {
    private static final String VM_CONFIG_JSON_FILE = "/data/local/tmp/vm_config.json";

    @Override
    public void onReceive(Context context, Intent intent) {
        PackageManager pm = context.getPackageManager();
        ComponentName compName =
                new ComponentName(context.getPackageName(), MainActivity.class.getCanonicalName());
        pm.setComponentEnabledSetting(
                compName,
                Files.exists(Path.of(VM_CONFIG_JSON_FILE))
                        ? PackageManager.COMPONENT_ENABLED_STATE_ENABLED
                        : PackageManager.COMPONENT_ENABLED_STATE_DISABLED,
                PackageManager.DONT_KILL_APP);
    }
}
