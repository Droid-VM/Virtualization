/*
 * Copyright 2022 The Android Open Source Project
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
import android.os.Parcel;
import android.os.ParcelFileDescriptor;
import android.os.Parcelable;

import com.android.internal.annotations.VisibleForTesting;

/** This class regroups a set of read-only file descriptors that represent the state of a VM. */
public class ParcelVirtualMachine implements Parcelable {
    private final ParcelFileDescriptor mConfigFd;
    private final ParcelFileDescriptor mInstanceImgFd;
    // TODO(b/243129654): Add trusted storage fd once it is available.

    public static final Parcelable.Creator<ParcelVirtualMachine> CREATOR =
            new Parcelable.Creator<ParcelVirtualMachine>() {
                public ParcelVirtualMachine createFromParcel(Parcel in) {
                    return new ParcelVirtualMachine(in);
                }

                public ParcelVirtualMachine[] newArray(int size) {
                    return new ParcelVirtualMachine[size];
                }
            };

    @Override
    public int describeContents() {
        return 0;
    }

    @Override
    public void writeToParcel(Parcel out, int flags) {
        mConfigFd.writeToParcel(out, flags);
        mInstanceImgFd.writeToParcel(out, flags);
    }

    /**
     * @return File descriptor of the VM configuration file config.xml.
     */
    @VisibleForTesting
    public @NonNull ParcelFileDescriptor getConfigFd() {
        return mConfigFd;
    }

    /**
     * @return File descriptor of the instance.img of the VM.
     */
    @VisibleForTesting
    public @NonNull ParcelFileDescriptor getInstanceImgFd() {
        return mInstanceImgFd;
    }

    ParcelVirtualMachine(
            @NonNull ParcelFileDescriptor configFd, @NonNull ParcelFileDescriptor instanceImgFd) {
        mConfigFd = configFd;
        mInstanceImgFd = instanceImgFd;
    }

    private ParcelVirtualMachine(Parcel in) {
        mConfigFd = in.readFileDescriptor();
        mInstanceImgFd = in.readFileDescriptor();
    }
}
