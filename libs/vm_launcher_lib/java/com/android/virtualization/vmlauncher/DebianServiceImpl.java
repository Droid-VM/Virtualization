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

package com.android.virtualization.vmlauncher;

import android.content.Context;
import android.content.SharedPreferences;
import android.util.Log;

import androidx.annotation.Keep;

import com.android.virtualization.vmlauncher.proto.DebianServiceGrpc;
import com.android.virtualization.vmlauncher.proto.ForwardingRequestItem;
import com.android.virtualization.vmlauncher.proto.IpAddr;
import com.android.virtualization.vmlauncher.proto.QueueOpeningRequest;
import com.android.virtualization.vmlauncher.proto.ReportVmIpAddrResponse;

import io.grpc.stub.StreamObserver;

import java.util.ArrayList;
import java.util.Collections;
import java.util.HashSet;
import java.util.Set;

final class DebianServiceImpl extends DebianServiceGrpc.DebianServiceImplBase {
    public static final String TAG = "DebianService";
    private SharedPreferences mSharedPref;
    private final DebianServiceCallback mCallback;

    static {
        System.loadLibrary("forwarder_host_jni");
    }

    protected DebianServiceImpl(Context appContext, DebianServiceCallback callback) {
        super();
        mCallback = callback;
        mSharedPref =
                appContext.getSharedPreferences(
                        "com.android.virtualization.terminal.PREFERENCE_FILE_KEY",
                        Context.MODE_PRIVATE);

        // TODO(b/340126051): Instead of putting fixed value, receive active port list info from the
        // guest.
        if (!mSharedPref.contains("PREFERENCE_FORWARDING_PORTS")) {
            SharedPreferences.Editor editor = mSharedPref.edit();
            Set<String> ports = new HashSet<>();
            for (int port = 8080; port < 8090; port++) {
                ports.add(Integer.toString(port));
                editor.putBoolean(
                        "PREFERENCE_FORWARDING_PORT_IS_ENABLED_" + Integer.toString(port), false);
            }
            editor.putStringSet("PREFERENCE_FORWARDING_PORTS", ports);
            editor.apply();
        }
    }

    @Override
    public void reportVmIpAddr(
            IpAddr request, StreamObserver<ReportVmIpAddrResponse> responseObserver) {
        Log.d(DebianServiceImpl.TAG, "reportVmIpAddr: " + request.toString());
        mCallback.onIpAddressAvailable(request.getAddr());
        ReportVmIpAddrResponse reply = ReportVmIpAddrResponse.newBuilder().setSuccess(true).build();
        responseObserver.onNext(reply);
        responseObserver.onCompleted();
    }

    @Override
    public void openForwardingRequestQueue(
            QueueOpeningRequest request, StreamObserver<ForwardingRequestItem> responseObserver) {
        Log.d(DebianServiceImpl.TAG, "OpenForwardingRequestQueue");
        mSharedPref.registerOnSharedPreferenceChangeListener(
                new SharedPreferences.OnSharedPreferenceChangeListener() {
                    @Override
                    public void onSharedPreferenceChanged(
                            SharedPreferences sharedPreferences, String key) {
                        if (key.startsWith("PREFERENCE_FORWARDING_PORT")) {
                            updateListeningPorts();
                        }
                    }
                });
        updateListeningPorts();
        runForwarderHost(request.getCid(), new ForwarderHostCallback(responseObserver));
        responseObserver.onCompleted();
    }

    @Keep
    private static class ForwarderHostCallback {
        private StreamObserver<ForwardingRequestItem> mResponseObserver;

        ForwarderHostCallback(StreamObserver<ForwardingRequestItem> responseObserver) {
            mResponseObserver = responseObserver;
        }

        private void onForwardingRequestReceived(int guestTcpPort, int vsockPort) {
            ForwardingRequestItem item =
                    ForwardingRequestItem.newBuilder()
                            .setGuestTcpPort(guestTcpPort)
                            .setVsockPort(vsockPort)
                            .build();
            mResponseObserver.onNext(item);
        }
    }

    private static native void runForwarderHost(int cid, ForwarderHostCallback callback);

    public static native void terminateForwarderHost();

    private static native void updateListeningPorts(int[] ports);

    private void updateListeningPorts() {
        Set<String> activePorts =
                mSharedPref.getStringSet("PREFERENCE_FORWARDING_PORTS", Collections.emptySet());
        ArrayList<Integer> listeningPorts = new ArrayList<Integer>();
        for (String port : activePorts) {
            if (mSharedPref.getBoolean("PREFERENCE_FORWARDING_PORT_IS_ENABLED_" + port, false)) {
                try {
                    listeningPorts.add(Integer.valueOf(port));
                } catch (NumberFormatException e) {
                    Log.e(DebianServiceImpl.TAG, "Failed to parse listening ports", e);
                }
            }
        }
        updateListeningPorts(listeningPorts.stream().mapToInt(Integer::intValue).toArray());
    }

    protected interface DebianServiceCallback {
        void onIpAddressAvailable(String ipAddr);
    }
}
