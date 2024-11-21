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
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.SharedPreferences;
import android.graphics.drawable.Icon;
import android.os.Bundle;
import android.os.IBinder;
import android.os.ResultReceiver;
import android.system.virtualmachine.VirtualMachine;
import android.system.virtualmachine.VirtualMachineConfig;
import android.system.virtualmachine.VirtualMachineCustomImageConfig;
import android.system.virtualmachine.VirtualMachineCustomImageConfig.Disk;
import android.system.virtualmachine.VirtualMachineException;
import android.util.Log;

import com.google.common.collect.Sets;

import io.grpc.Grpc;
import io.grpc.InsecureServerCredentials;
import io.grpc.Metadata;
import io.grpc.Server;
import io.grpc.ServerCall;
import io.grpc.ServerCallHandler;
import io.grpc.ServerInterceptor;
import io.grpc.Status;
import io.grpc.okhttp.OkHttpServerBuilder;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.nio.file.Path;
import java.util.Collections;
import java.util.HashSet;
import java.util.Objects;
import java.util.Set;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public class VmLauncherService extends Service implements DebianServiceImpl.DebianServiceCallback {
    public static final String EXTRA_NOTIFICATION = "EXTRA_NOTIFICATION";
    static final String TAG = "VmLauncherService";

    private static final int RESULT_START = 0;
    private static final int RESULT_STOP = 1;
    private static final int RESULT_ERROR = 2;
    private static final int RESULT_IPADDR = 3;
    private static final String KEY_VM_IP_ADDR = "ip_addr";

    private ExecutorService mExecutorService;
    private VirtualMachine mVirtualMachine;
    private ResultReceiver mResultReceiver;
    private Server mServer;
    private DebianServiceImpl mDebianService;
    private final IntentFilter mIntentFilter = new IntentFilter();

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private void startForeground(Notification notification) {
        startForeground(this.hashCode(), notification);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (Objects.equals(
                intent.getAction(), VmLauncherServices.ACTION_STOP_VM_LAUNCHER_SERVICE)) {
            stopSelf();
            return START_NOT_STICKY;
        }
        if (mVirtualMachine != null) {
            Log.d(TAG, "VM instance is already started");
            return START_NOT_STICKY;
        }
        mExecutorService = Executors.newCachedThreadPool();

        ConfigJson json = ConfigJson.from(InstallUtils.getVmConfigPath(this));
        VirtualMachineConfig.Builder configBuilder = json.toConfigBuilder(this);
        VirtualMachineCustomImageConfig.Builder customImageConfigBuilder =
                json.toCustomImageConfigBuilder(this);
        File backupFile = InstallUtils.getBackupFile(this);
        if (backupFile.exists()) {
            customImageConfigBuilder.addDisk(Disk.RWDisk(backupFile.getPath()));
            configBuilder.setCustomImageConfig(customImageConfigBuilder.build());
        }
        VirtualMachineConfig config = configBuilder.build();

        Runner runner;
        try {
            android.os.Trace.beginSection("vmCreate");
            runner = Runner.create(this, config);
            android.os.Trace.endSection();
            android.os.Trace.beginAsyncSection("debianBoot", 0);
        } catch (VirtualMachineException e) {
            Log.e(TAG, "cannot create runner", e);
            stopSelf();
            return START_NOT_STICKY;
        }
        mVirtualMachine = runner.getVm();
        mResultReceiver =
                intent.getParcelableExtra(Intent.EXTRA_RESULT_RECEIVER, ResultReceiver.class);

        runner.getExitStatus()
                .thenAcceptAsync(
                        success -> {
                            if (mResultReceiver != null) {
                                mResultReceiver.send(success ? RESULT_STOP : RESULT_ERROR, null);
                            }
                            stopSelf();
                        });
        Path logPath = getFileStreamPath(mVirtualMachine.getName() + ".log").toPath();
        Logger.setup(mVirtualMachine, logPath, mExecutorService);

        Notification notification =
                intent.getParcelableExtra(EXTRA_NOTIFICATION, Notification.class);

        startForeground(notification);

        mResultReceiver.send(RESULT_START, null);

        preparePortForwardingNotifications();
        startDebianServer();

        return START_NOT_STICKY;
    }

    @Override
    public void onDestroy() {
        super.onDestroy();
        stopDebianServer();
        if (mVirtualMachine != null) {
            if (mVirtualMachine.getStatus() == VirtualMachine.STATUS_RUNNING) {
                try {
                    mVirtualMachine.stop();
                    stopForeground(STOP_FOREGROUND_REMOVE);
                } catch (VirtualMachineException e) {
                    Log.e(TAG, "failed to stop a VM instance", e);
                }
            }
            mExecutorService.shutdownNow();
            mExecutorService = null;
            mVirtualMachine = null;
        }
    }

    private void startDebianServer() {
        ServerInterceptor interceptor =
                new ServerInterceptor() {
                    @Override
                    public <ReqT, RespT> ServerCall.Listener<ReqT> interceptCall(
                            ServerCall<ReqT, RespT> call,
                            Metadata headers,
                            ServerCallHandler<ReqT, RespT> next) {
                        // Refer to VirtualizationSystemService.TetheringService
                        final String VM_STATIC_IP_ADDR = "192.168.0.2";
                        InetSocketAddress remoteAddr =
                                (InetSocketAddress)
                                        call.getAttributes().get(Grpc.TRANSPORT_ATTR_REMOTE_ADDR);

                        if (remoteAddr != null
                                && Objects.equals(
                                        remoteAddr.getAddress().getHostAddress(),
                                        VM_STATIC_IP_ADDR)) {
                            // Allow the request only if it is from VM
                            return next.startCall(call, headers);
                        }
                        Log.d(TAG, "blocked grpc request from " + remoteAddr);
                        call.close(Status.Code.PERMISSION_DENIED.toStatus(), new Metadata());
                        return new ServerCall.Listener<ReqT>() {};
                    }
                };
        try {
            // TODO(b/372666638): gRPC for java doesn't support vsock for now.
            int port = 0;
            mDebianService = new DebianServiceImpl(this, this);
            mServer =
                    OkHttpServerBuilder.forPort(port, InsecureServerCredentials.create())
                            .intercept(interceptor)
                            .addService(mDebianService)
                            .build()
                            .start();
        } catch (IOException e) {
            Log.d(TAG, "grpc server error", e);
            return;
        }

        mExecutorService.execute(
                () -> {
                    // TODO(b/373533555): we can use mDNS for that.
                    String debianServicePortFileName = "debian_service_port";
                    File debianServicePortFile = new File(getFilesDir(), debianServicePortFileName);
                    try (FileOutputStream writer = new FileOutputStream(debianServicePortFile)) {
                        writer.write(String.valueOf(mServer.getPort()).getBytes());
                    } catch (IOException e) {
                        Log.d(TAG, "cannot write grpc port number", e);
                    }
                });
    }

    private void stopDebianServer() {
        if (mDebianService != null) {
            mDebianService.killForwarderHost();
        }
        if (mServer != null) {
            mServer.shutdown();
        }
    }

    @Override
    public void onIpAddressAvailable(String ipAddr) {
        android.os.Trace.endAsyncSection("debianBoot", 0);
        Bundle b = new Bundle();
        b.putString(VmLauncherService.KEY_VM_IP_ADDR, ipAddr);
        mResultReceiver.send(VmLauncherService.RESULT_IPADDR, b);
    }

    private void showPortForwardingNotification(int port) {
        Intent tapIntent = new Intent(this, SettingsPortForwardingActivity.class);
        tapIntent.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        PendingIntent tapPendingIntent =
                PendingIntent.getActivity(this, 0, tapIntent, PendingIntent.FLAG_IMMUTABLE);

        Intent accessIntent = new Intent(this, PortForwardingRequestReceiver.class);
        accessIntent.setAction(PortForwardingRequestReceiver.ACTION_PORT_FORWARDING);
        accessIntent.setIdentifier("access_" + Integer.toString(port));
        accessIntent.putExtra(PortForwardingRequestReceiver.KEY_PORT, port);
        accessIntent.putExtra(PortForwardingRequestReceiver.KEY_ENABLED, true);
        PendingIntent accessPendingIntent =
                PendingIntent.getBroadcast(this, 0, accessIntent, PendingIntent.FLAG_IMMUTABLE);

        Intent denyIntent = new Intent(this, PortForwardingRequestReceiver.class);
        denyIntent.setAction(PortForwardingRequestReceiver.ACTION_PORT_FORWARDING);
        denyIntent.setIdentifier("deny_" + Integer.toString(port));
        denyIntent.putExtra(PortForwardingRequestReceiver.KEY_PORT, port);
        denyIntent.putExtra(PortForwardingRequestReceiver.KEY_ENABLED, false);
        PendingIntent denyPendingIntent =
                PendingIntent.getBroadcast(this, 0, denyIntent, PendingIntent.FLAG_IMMUTABLE);

        String notificationTitle = getString(R.string.settings_port_forwarding_notification_title);
        String notificationContent =
                getString(R.string.settings_port_forwarding_notification_content, port);
        String notificationAcceptButtonText =
                getString(R.string.settings_port_forwarding_notification_accept);
        String notificationDenyButtonText =
                getString(R.string.settings_port_forwarding_notification_deny);
        Icon icon = Icon.createWithResource(this, R.drawable.ic_launcher_foreground);

        Notification notification =
                new Notification.Builder(this, this.getPackageName())
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

        getSystemService(NotificationManager.class).notify(TAG, port, notification);
    }

    private void discardPortForwardingNotification(int port) {
        getSystemService(NotificationManager.class).cancel(TAG, port);
    }

    private void preparePortForwardingNotifications() {
        SharedPreferences sharedPref =
                this.getSharedPreferences(
                        getString(R.string.preference_file_key), Context.MODE_PRIVATE);
        sharedPref.registerOnSharedPreferenceChangeListener(new PortForwardingListener());
    }

    private final class PortForwardingListener
            implements SharedPreferences.OnSharedPreferenceChangeListener {
        private Set<String> mPrevActivePorts;

        PortForwardingListener() {
            mPrevActivePorts = new HashSet<>();
        }

        @Override
        public void onSharedPreferenceChanged(SharedPreferences sharedPref, String key) {
            if (!key.equals(getString(R.string.preference_forwarding_ports))) {
                return;
            }
            Set<String> activePorts = sharedPref.getStringSet(key, Collections.emptySet());
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
    }
}
