/*
 * Copyright 2023 The Android Open Source Project
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

package com.android.microdroid.test.host;

import static java.nio.ByteOrder.LITTLE_ENDIAN;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.util.Objects;
import java.nio.ByteBuffer;

/** pvmfw.bin with custom config payloads on host. */
public final class Pvmfw {
    private static final int SIZE_8B = 8; // 8 bytes
    private static final int SIZE_4K = 4 << 10; // 4 KiB, PAGE_SIZE
    private static final int BUFFER_SIZE = 1024;
    private static final int HEADER_SIZE = Integer.BYTES * 8; // Header has 8 integers.
    private static final int HEADER_MAGIC = 0x666d7670;
    private static final int HEADER_DEFAULT_VERSION = getVersion(1, 0);
    private static final int HEADER_FLAGS = 0;

    @NonNull private final File mPvmfwBinFile;
    @NonNull private final File mBccFile;
    @Nullable private final File mDebugPolicyFile;
    private final int mVersion;

    private Pvmfw(
            @NonNull File pvmfwBinFile,
            @NonNull File bccFile,
            @Nullable File debugPolicyFile,
            int version) {
        mPvmfwBinFile = Objects.requireNonNull(pvmfwBinFile);
        mBccFile = Objects.requireNonNull(bccFile);
        mDebugPolicyFile = debugPolicyFile;
        mVersion = version;
    }

    /**
     * Serializes pvmfw.bin and its config, as written in the <a
     * href="https://android.googlesource.com/platform/packages/modules/Virtualization/+/master/pvmfw/README.md">README.md</a>
     */
    public void serialize(@NonNull File outFile) throws IOException {
        Objects.requireNonNull(outFile);

        int bccOffset = HEADER_SIZE;
        int bccSize = (int) mBccFile.length();

        int debugPolicyOffset = alignTo(bccOffset + bccSize, SIZE_8B);
        int debugPolicySize = mDebugPolicyFile == null ? 0 : (int) mDebugPolicyFile.length();

        int totalSize = debugPolicyOffset + debugPolicySize;

        ByteBuffer header = ByteBuffer.allocate(HEADER_SIZE).order(LITTLE_ENDIAN);
        header.putInt(HEADER_MAGIC);
        header.putInt(mVersion);
        header.putInt(totalSize);
        header.putInt(HEADER_FLAGS);
        header.putInt(bccOffset);
        header.putInt(bccSize);
        header.putInt(debugPolicyOffset);
        header.putInt(debugPolicySize);

        if (hasVmDtbo(mVersion)) {
            // Add placeholder entry for dummy VM DTBO.
            // TODO(jaewan): Add tests for VM DTBO instead of adding dummy offsets.
            header.putInt(0);
            header.putInt(0);
        }

        try (FileOutputStream pvmfw = new FileOutputStream(outFile)) {
            appendFile(pvmfw, mPvmfwBinFile);
            padTo(pvmfw, SIZE_4K);
            pvmfw.write(header.array());
            padTo(pvmfw, HEADER_SIZE);
            appendFile(pvmfw, mBccFile);
            if (mDebugPolicyFile != null) {
                padTo(pvmfw, SIZE_8B);
                appendFile(pvmfw, mDebugPolicyFile);
            }
            padTo(pvmfw, SIZE_4K);
        }
    }

    private void appendFile(@NonNull FileOutputStream out, @NonNull File inFile)
            throws IOException {
        byte buffer[] = new byte[BUFFER_SIZE];
        try (FileInputStream in = new FileInputStream(inFile)) {
            int size;
            while (true) {
                size = in.read(buffer);
                if (size < 0) {
                    return;
                }
                out.write(buffer, /* offset= */ 0, size);
            }
        }
    }

    private void padTo(@NonNull FileOutputStream out, int size) throws IOException {
        int streamSize = (int) out.getChannel().size();
        for (int i = streamSize; i < alignTo(streamSize, size); i++) {
            out.write(0); // write byte.
        }
    }

    private static boolean hasVmDtbo(int version) {
        int major = getMajorVersion(version);
        int minor = getMinorVersion(version);
        return major > 1 || (major == 1 && minor >= 1);
    }

    private static int alignTo(int x, int size) {
        return (x + size - 1) & ~(size - 1);
    }

    private static int getVersion(int major, int minor) {
        return ((major & 0xFFFF) << 16) | (minor & 0xFFFF);
    }

    private static int getMajorVersion(int version) {
        return (version >> 16) & 0xFFFF;
    }

    private static int getMinorVersion(int version) {
        return version & 0xFFFF;
    }

    /** Builder for {@link Pvmfw}. */
    public static final class Builder {
        @NonNull private final File mPvmfwBinFile;
        @NonNull private final File mBccFile;
        @Nullable private File mDebugPolicyFile;
        private int mVersion;

        public Builder(@NonNull File pvmfwBinFile, @NonNull File bccFile) {
            mPvmfwBinFile = Objects.requireNonNull(pvmfwBinFile);
            mBccFile = Objects.requireNonNull(bccFile);
            mVersion = HEADER_DEFAULT_VERSION;
        }

        @NonNull
        public Builder setDebugPolicyOverlay(@Nullable File debugPolicyFile) {
            mDebugPolicyFile = debugPolicyFile;
            return this;
        }

        @NonNull
        public Builder setVersion(int major, int minor) {
            mVersion = getVersion(major, minor);
            return this;
        }

        @NonNull
        public Pvmfw build() {
            return new Pvmfw(mPvmfwBinFile, mBccFile, mDebugPolicyFile, mVersion);
        }
    }
}
