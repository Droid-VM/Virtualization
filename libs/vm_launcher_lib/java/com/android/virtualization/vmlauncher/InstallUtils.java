package com.android.virtualization.vmlauncher;

import android.content.Context;
import android.os.Environment;
import android.util.Log;

import org.apache.commons.compress.archivers.ArchiveEntry;
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream;
import org.apache.commons.compress.compressors.gzip.GzipCompressorInputStream;

import java.io.BufferedInputStream;
import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.HashMap;
import java.util.Map;
import java.util.function.Function;

public class InstallUtils {
    private static final String TAG = InstallUtils.class.getSimpleName();

    // TODO: this path should be from outside of this service
    private static final String VM_CONFIG_FILENAME = "vm_config.json";
    private static final String ARTIFACT_FILENAME = "images.tar.gz";

    public static String getVmConfigPath(Context context) {
        return context.getFileStreamPath(VM_CONFIG_FILENAME).getPath();
    }

    public static boolean isImageInstalled(Context context) {
        return Files.exists(Path.of(getVmConfigPath(context)));
    }

    public static boolean installImageFromExternalStorage(Context context) {
        File artifactDir = Environment.getExternalStoragePublicDirectory("linux");
        if (artifactDir == null) {
            Log.d(TAG, "no artifact dir: " + artifactDir);
            return false;
        }
        Path artifactPath = artifactDir.toPath().resolve(ARTIFACT_FILENAME);
        if (!Files.exists(artifactPath)) {
            Log.d(TAG, "no artifact file: " + artifactPath);
            return false;
        }

        try (BufferedInputStream inputStream =
                        new BufferedInputStream(Files.newInputStream(artifactPath));
                TarArchiveInputStream tar =
                        new TarArchiveInputStream(new GzipCompressorInputStream(inputStream))) {
            ArchiveEntry entry;
            while ((entry = tar.getNextEntry()) != null) {
                Path extractTo = context.getFilesDir().toPath().resolve(entry.getName());
                if (entry.isDirectory()) {
                    Files.createDirectories(extractTo);
                } else {
                    Files.copy(tar, extractTo, StandardCopyOption.REPLACE_EXISTING);
                }
            }
        } catch (IOException e) {
            Log.e(TAG, "installation failed", e);
            return false;
        }
        if (!isImageInstalled(context)) {
            return false;
        }

        if (!resolvePathInVmConfig(context)) {
            Log.d(TAG, "resolving path failed");
            try {
                Files.deleteIfExists(Path.of(getVmConfigPath(context)));
            } catch (IOException e) {
                return false;
            }
            return false;
        }
        return true;
    }

    private static Function<String, String> getReplacer(Context context) {
        Map<String, String> rules = new HashMap<>();
        rules.put("\\$DATA_DIR", context.getFilesDir().toString());
        return (s) -> {
            for (Map.Entry<String, String> rule : rules.entrySet()) {
                Log.d(TAG, s);
                s = s.replaceAll(rule.getKey(), rule.getValue());
                Log.d(TAG, s);
            }
            return s;
        };
    }

    private static boolean resolvePathInVmConfig(Context context) {
        try {
            String replacedVmConfig =
                    String.join(
                            "\n",
                            Files.readAllLines(Path.of(getVmConfigPath(context))).stream()
                                    .map(getReplacer(context))
                                    .toList());
            Files.write(Path.of(getVmConfigPath(context)), replacedVmConfig.getBytes());
            return true;
        } catch (IOException e) {
            return false;
        }
    }
}
