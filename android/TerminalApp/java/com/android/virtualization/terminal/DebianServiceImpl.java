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

package com.android.virtualization.terminal;

import android.app.Notification;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.res.Resources;
import android.graphics.drawable.Icon;
import android.util.Log;

import androidx.annotation.Keep;

import com.android.virtualization.terminal.proto.DebianServiceGrpc;
import com.android.virtualization.terminal.proto.ForwardingRequestItem;
import com.android.virtualization.terminal.proto.IpAddr;
import com.android.virtualization.terminal.proto.QueueOpeningRequest;
import com.android.virtualization.terminal.proto.ReportVmActivePortsRequest;
import com.android.virtualization.terminal.proto.ReportVmActivePortsResponse;
import com.android.virtualization.terminal.proto.ReportVmIpAddrResponse;

import com.google.common.collect.Sets;

import io.grpc.stub.StreamObserver;

import java.util.Collections;
import java.util.HashSet;
import java.util.Set;

final class DebianServiceImpl extends DebianServiceGrpc.DebianServiceImplBase {
    public static final String TAG = "DebianService";
    private static final String PREFERENCE_FORWARDING_PORT_IS_ENABLED_PREFIX =
            "PREFERENCE_FORWARDING_PORT_IS_ENABLED_";

    private final Context mContext;
    private final Resources mResources;
    private final SharedPreferences mSharedPref;
    private SharedPreferences.OnSharedPreferenceChangeListener mPortForwardingListener;
    private Set<String> mPrevActivePorts;
    private final DebianServiceCallback mCallback;

    static {
        System.loadLibrary("forwarder_host_jni");
    }

    DebianServiceImpl(Context context, DebianServiceCallback callback) {
        super();
        mCallback = callback;
        mContext = context;
        mResources = context.getResources();
        mSharedPref =
                mContext.getSharedPreferences(
                        mContext.getString(R.string.preference_file_key), Context.MODE_PRIVATE);
        mPrevActivePorts = new HashSet<>();
    }

    @Override
    public void reportVmActivePorts(
            ReportVmActivePortsRequest request,
            StreamObserver<ReportVmActivePortsResponse> responseObserver) {
        Log.d(DebianServiceImpl.TAG, "reportVmActivePorts: " + request.toString());

        SharedPreferences.Editor editor = mSharedPref.edit();
        Set<String> ports = new HashSet<>();
        for (int port : request.getPortsList()) {
            ports.add(Integer.toString(port));
            if (!mSharedPref.contains(
                    mContext.getString(R.string.preference_forwarding_port_is_enabled)
                            + Integer.toString(port))) {
                editor.putBoolean(
                        mContext.getString(R.string.preference_forwarding_port_is_enabled)
                                + Integer.toString(port),
                        false);
            }
        }
        editor.putStringSet(mResources.getString(R.string.preference_forwarding_ports), ports);
        editor.apply();

        ReportVmActivePortsResponse reply =
                ReportVmActivePortsResponse.newBuilder().setSuccess(true).build();
        responseObserver.onNext(reply);
        responseObserver.onCompleted();
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
        mPortForwardingListener =
                new SharedPreferences.OnSharedPreferenceChangeListener() {
                    @Override
                    public void onSharedPreferenceChanged(
                            SharedPreferences sharedPreferences, String key) {
                        if (key.startsWith(
                                        mContext.getString(
                                                R.string.preference_forwarding_port_is_enabled))
                                || key.equals(
                                        mResources.getString(
                                                R.string.preference_forwarding_ports))) {
                            updateListeningPorts();
                        }
                        if (key.equals(
                                mResources.getString(R.string.preference_forwarding_ports))) {
                            preparePortForwardingNotifications();
                        }
                    }
                };
        mSharedPref.registerOnSharedPreferenceChangeListener(mPortForwardingListener);
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

    private static native void terminateForwarderHost();

    void killForwarderHost() {
        Log.d(DebianServiceImpl.TAG, "Stopping port forwarding");
        if (mPortForwardingListener != null) {
            mSharedPref.unregisterOnSharedPreferenceChangeListener(mPortForwardingListener);
            terminateForwarderHost();
        }
    }

    private static native void updateListeningPorts(int[] ports);

    private void updateListeningPorts() {
        updateListeningPorts(
                mSharedPref
                        .getStringSet(
                                mResources.getString(R.string.preference_forwarding_ports),
                                Collections.emptySet())
                        .stream()
                        .filter(
                                port ->
                                        mSharedPref.getBoolean(
                                                PREFERENCE_FORWARDING_PORT_IS_ENABLED_PREFIX + port,
                                                false))
                        .map(Integer::valueOf)
                        .mapToInt(Integer::intValue)
                        .toArray());
    }

    private void showPortForwardingNotification(int port) {
        Intent tapIntent = new Intent(mContext, SettingsPortForwardingActivity.class);
        tapIntent.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        PendingIntent tapPendingIntent =
                PendingIntent.getActivity(mContext, 0, tapIntent, PendingIntent.FLAG_IMMUTABLE);

        Intent accessIntent = new Intent(mContext, PortForwardingRequestReceiver.class);
        accessIntent.setAction(PortForwardingRequestReceiver.ACTION_PORT_FORWARDING);
        accessIntent.setIdentifier("access_" + Integer.toString(port));
        accessIntent.putExtra("port", port);
        accessIntent.putExtra("enabled", true);
        PendingIntent accessPendingIntent =
                PendingIntent.getBroadcast(mContext, 0, accessIntent, PendingIntent.FLAG_IMMUTABLE);

        Intent denyIntent = new Intent(mContext, PortForwardingRequestReceiver.class);
        denyIntent.setAction(PortForwardingRequestReceiver.ACTION_PORT_FORWARDING);
        denyIntent.setIdentifier("deny_" + Integer.toString(port));
        denyIntent.putExtra("port", port);
        denyIntent.putExtra("enabled", false);
        PendingIntent denyPendingIntent =
                PendingIntent.getBroadcast(mContext, 0, denyIntent, PendingIntent.FLAG_IMMUTABLE);

        Icon icon = Icon.createWithResource(mResources, R.drawable.ic_launcher_foreground);
        String notificationTitle =
                mResources.getString(R.string.settings_port_forwarding_notification_title);
        String notificationContent =
                mResources.getString(R.string.settings_port_forwarding_notification_content, port);
        String notificationAcceptButtonText =
                mResources.getString(R.string.settings_port_forwarding_notification_accept);
        String notificationDenyButtonText =
                mResources.getString(R.string.settings_port_forwarding_notification_deny);

        Notification notification =
                new Notification.Builder(mContext, mContext.getPackageName())
                        .setSmallIcon(R.drawable.ic_launcher_foreground)
                        .setContentTitle(notificationTitle)
                        .setContentText(notificationContent)
                        .setContentIntent(tapPendingIntent)
                        .addAction(
                                new Notification.Action.Builder(
                                                icon,
                                                notificationAcceptButtonText,
                                                accessPendingIntent)
                                        .build())
                        .addAction(
                                new Notification.Action.Builder(
                                                icon, notificationDenyButtonText, denyPendingIntent)
                                        .build())
                        .build();

        mContext.getSystemService(NotificationManager.class).notify(TAG, port, notification);
    }

    private void discardPortForwardingNotification(int port) {
        mContext.getSystemService(NotificationManager.class).cancel(TAG, port);
    }

    private void preparePortForwardingNotifications() {
        Set<String> activePorts =
                mSharedPref.getStringSet(
                        mResources.getString(R.string.preference_forwarding_ports),
                        Collections.emptySet());
        for (String portStr : Sets.difference(activePorts, mPrevActivePorts)) {
            try {
                int port = Integer.parseInt(portStr);
                showPortForwardingNotification(port);
            } catch (NumberFormatException e) {
                Log.e(TAG, "Failed to parse port: " + portStr);
                return;
            }
        }
        for (String portStr : Sets.difference(mPrevActivePorts, activePorts)) {
            try {
                int port = Integer.parseInt(portStr);
                discardPortForwardingNotification(port);
            } catch (NumberFormatException e) {
                Log.e(TAG, "Failed to parse port: " + portStr);
                return;
            }
        }
        mPrevActivePorts = activePorts;
    }

    protected interface DebianServiceCallback {
        void onIpAddressAvailable(String ipAddr);
    }
}
