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

package android.system.virtualmachine;

import android.annotation.NonNull;
import android.annotation.Nullable;
import android.os.PersistableBundle;

import android.system.virtualizationservice.DisplayConfig;

import java.util.ArrayList;
import java.util.List;

/** @hide */
public class VirtualMachineCustomImageConfig {
    private static final String KEY_NAME = "name";
    private static final String KEY_KERNEL = "kernel";
    private static final String KEY_INITRD = "initrd";
    private static final String KEY_BOOTLOADER = "bootloader";
    private static final String KEY_PARAMS = "params";
    private static final String KEY_DISK_WRITABLES = "disk_writables";
    private static final String KEY_DISK_IMAGES = "disk_images";
    private static final String KEY_DISPLAY_CONFIG = "display_config";
    private static final String KEY_DISPLAY_CONFIG_WIDTH = "display_config_width";
    private static final String KEY_DISPLAY_CONFIG_HEIGHT = "display_config_width";
    private static final String KEY_DISPLAY_CONFIG_HORIZONTAL_DPI = "display_config_horizontal_dpi";
    private static final String KEY_DISPLAY_CONFIG_VERTICAL_DPI = "display_config_vertical_dpi";
    private static final String KEY_DISPLAY_CONFIG_REFRESH_RATE = "display_config_refresh_rate";

    @Nullable private final String name;
    @NonNull private final String kernelPath;
    @Nullable private final String initrdPath;
    @Nullable private final String bootloaderPath;
    @Nullable private final String[] params;
    @Nullable private final Disk[] disks;
    @Nullable private final DisplayConfig displayConfig;

    @Nullable
    public Disk[] getDisks() {
        return disks;
    }

    @Nullable
    public String getBootloaderPath() {
        return bootloaderPath;
    }

    @Nullable
    public String getInitrdPath() {
        return initrdPath;
    }

    @NonNull
    public String getKernelPath() {
        return kernelPath;
    }

    @Nullable
    public String getName() {
        return name;
    }

    @Nullable
    public String[] getParams() {
        return params;
    }

    /** @hide */
    public VirtualMachineCustomImageConfig(
            String name,
            String kernelPath,
            String initrdPath,
            String bootloaderPath,
            String[] params,
            Disk[] disks,
            DisplayConfig displayConfig) {
        this.name = name;
        this.kernelPath = kernelPath;
        this.initrdPath = initrdPath;
        this.bootloaderPath = bootloaderPath;
        this.params = params;
        this.disks = disks;
        this.displayConfig = displayConfig;
    }

    static VirtualMachineCustomImageConfig from(PersistableBundle customImageConfigBundle) {
        Builder builder = new Builder();
        builder.setName(customImageConfigBundle.getString(KEY_NAME));
        builder.setKernelPath(customImageConfigBundle.getString(KEY_KERNEL));
        builder.setInitrdPath(customImageConfigBundle.getString(KEY_INITRD));
        builder.setBootloaderPath(customImageConfigBundle.getString(KEY_BOOTLOADER));
        String[] params = customImageConfigBundle.getStringArray(KEY_PARAMS);
        if (params != null) {
            for (String param : params) {
                builder.addParam(param);
            }
        }
        boolean[] writables = customImageConfigBundle.getBooleanArray(KEY_DISK_WRITABLES);
        String[] diskImages = customImageConfigBundle.getStringArray(KEY_DISK_IMAGES);
        if (writables != null && diskImages != null) {
            if (writables.length == diskImages.length) {
                for (int i = 0; i < writables.length; i++) {
                    builder.addDisk(
                            writables[i] ? Disk.RWDisk(diskImages[i]) : Disk.RODisk(diskImages[i]));
                }
            }
        }
        PersistableBundle displayConfigPb =
                customImageConfigBundle.getPersistableBundle(KEY_DISPLAY_CONFIG);
        builder.setDisplayConfig(createDisplayConfigFrom(displayConfigPb));

        return builder.build();
    }

    private static DisplayConfig createDisplayConfigFrom(PersistableBundle pb) {
        if (pb == null) {
            return null;
        }
        DisplayConfig displayConfig = new DisplayConfig();
        displayConfig.width = pb.getInt(KEY_DISPLAY_CONFIG_WIDTH);
        displayConfig.height = pb.getInt(KEY_DISPLAY_CONFIG_HEIGHT);
        displayConfig.horizontalDpi = pb.getInt(KEY_DISPLAY_CONFIG_HORIZONTAL_DPI);
        displayConfig.verticalDpi = pb.getInt(KEY_DISPLAY_CONFIG_VERTICAL_DPI);
        displayConfig.refreshRate = pb.getInt(KEY_DISPLAY_CONFIG_REFRESH_RATE);
        return displayConfig;
    }

    private static PersistableBundle displayConfigToPersistableBundle(DisplayConfig displayConfig) {
        if (displayConfig == null) {
            return null;
        }
        PersistableBundle pb = new PersistableBundle();
        pb.putInt(KEY_DISPLAY_CONFIG_WIDTH, displayConfig.width);
        pb.putInt(KEY_DISPLAY_CONFIG_HEIGHT, displayConfig.height);
        pb.putInt(KEY_DISPLAY_CONFIG_HORIZONTAL_DPI, displayConfig.horizontalDpi);
        pb.putInt(KEY_DISPLAY_CONFIG_VERTICAL_DPI, displayConfig.verticalDpi);
        pb.putInt(KEY_DISPLAY_CONFIG_REFRESH_RATE, displayConfig.refreshRate);
        return pb;
    }

    PersistableBundle toPersistableBundle() {
        PersistableBundle pb = new PersistableBundle();
        pb.putString(KEY_NAME, this.name);
        pb.putString(KEY_KERNEL, this.kernelPath);
        pb.putString(KEY_BOOTLOADER, this.bootloaderPath);
        pb.putString(KEY_INITRD, this.initrdPath);
        pb.putStringArray(KEY_PARAMS, this.params);

        if (disks != null) {
            boolean[] writables = new boolean[disks.length];
            String[] images = new String[disks.length];
            for (int i = 0; i < disks.length; i++) {
                writables[i] = disks[i].writable;
                images[i] = disks[i].imagePath;
            }
            pb.putBooleanArray(KEY_DISK_WRITABLES, writables);
            pb.putStringArray(KEY_DISK_IMAGES, images);
        }
        pb.putPersistableBundle(
                KEY_DISPLAY_CONFIG, displayConfigToPersistableBundle(displayConfig));
        return pb;
    }

    @Nullable
    public DisplayConfig getDisplayConfig() {
        return displayConfig;
    }

    /** @hide */
    public static final class Disk {
        private final boolean writable;
        private final String imagePath;

        private Disk(boolean writable, String imagePath) {
            this.writable = writable;
            this.imagePath = imagePath;
        }

        /** @hide */
        public static Disk RWDisk(String imagePath) {
            return new Disk(true, imagePath);
        }

        /** @hide */
        public static Disk RODisk(String imagePath) {
            return new Disk(false, imagePath);
        }

        /** @hide */
        public boolean isWritable() {
            return writable;
        }

        /** @hide */
        public String getImagePath() {
            return imagePath;
        }
    }

    /** @hide */
    public static final class Builder {
        private String name;
        private String kernelPath;
        private String initrdPath;
        private String bootloaderPath;
        private List<String> params = new ArrayList<>();
        private List<Disk> disks = new ArrayList<>();
        private DisplayConfig displayConfig;

        /** @hide */
        public Builder() {}

        /** @hide */
        public Builder setName(String name) {
            this.name = name;
            return this;
        }

        /** @hide */
        public Builder setKernelPath(String kernelPath) {
            this.kernelPath = kernelPath;
            return this;
        }

        /** @hide */
        public Builder setBootloaderPath(String bootloaderPath) {
            this.bootloaderPath = bootloaderPath;
            return this;
        }

        /** @hide */
        public Builder setInitrdPath(String initrdPath) {
            this.initrdPath = initrdPath;
            return this;
        }

        /** @hide */
        public Builder addDisk(Disk disk) {
            this.disks.add(disk);
            return this;
        }

        /** @hide */
        public Builder addParam(String param) {
            this.params.add(param);
            return this;
        }

        /** @hide */
        public Builder setDisplayConfig(DisplayConfig displayConfig) {
            this.displayConfig = displayConfig;
            return this;
        }

        /** @hide */
        public VirtualMachineCustomImageConfig build() {
            return new VirtualMachineCustomImageConfig(
                    this.name,
                    this.kernelPath,
                    this.initrdPath,
                    this.bootloaderPath,
                    this.params.toArray(new String[0]),
                    this.disks.toArray(new Disk[0]),
                    displayConfig);
        }
    }
}
