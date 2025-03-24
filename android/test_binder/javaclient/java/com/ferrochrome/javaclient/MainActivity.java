package com.ferrochrome.javaclient;

import android.content.ComponentName;
import android.content.Intent;
import android.content.ServiceConnection;
import android.os.Bundle;
import android.os.IBinder;
import android.os.IBinder.DeathRecipient;
import android.os.RemoteException;
import android.os.ServiceManager;
import android.util.Log;
import android.widget.TextView;

import androidx.activity.EdgeToEdge;
import androidx.annotation.MainThread;
import androidx.annotation.NonNull;
import androidx.appcompat.app.AppCompatActivity;
import androidx.core.graphics.Insets;
import androidx.core.view.ViewCompat;
import androidx.core.view.WindowInsetsCompat;

import com.ferrochrome.IInstance;
import com.ferrochrome.IService;

import java.lang.ref.WeakReference;

public class MainActivity extends AppCompatActivity {
    private static final String TAG = "JavaClient";
    private MyServiceConnection serviceConnection;
    private DeathRecipient deathRecipientSvc;
    private DeathRecipient deathRecipientInst;
    private IService service;
    private IInstance instance;
    private TextView logView;
    private String logs;

    static {
        System.loadLibrary("test_binder_jni");
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        EdgeToEdge.enable(this);
        setContentView(R.layout.activity_main);
        ViewCompat.setOnApplyWindowInsetsListener(
                findViewById(R.id.main),
                (v, insets) -> {
                    Insets systemBars = insets.getInsets(WindowInsetsCompat.Type.systemBars());
                    v.setPadding(
                            systemBars.left, systemBars.top, systemBars.right, systemBars.bottom);
                    return insets;
                });

        deathRecipientInst =
                new DeathRecipient() {
                    @Override
                    public void binderDied() {
                        printLog("unexpected binderDied().. expects binderDied(IBinder)");
                    }

                    @Override
                    public void binderDied(@NonNull IBinder who) {
                        printLog("Instance Binder died..");
                        handleDisconnect();
                    }
                };
        deathRecipientSvc =
                new DeathRecipient() {
                    @Override
                    public void binderDied() {
                        printLog("unexpected binderDied().. expects binderDied(IBinder)");
                    }

                    @Override
                    public void binderDied(@NonNull IBinder who) {
                        printLog("SVC Binder died..");
                        handleDisconnect();
                    }
                };

        logView = findViewById(R.id.logs);
        logs = "";

        findViewById(R.id.connect_to_java)
                .setOnClickListener(
                        (v) -> {
                            connectToService(
                                    "com.ferrochrome.javabackend",
                                    "com.ferrochrome.javabackend.MainService");
                        });

        findViewById(R.id.connect_to_rust)
                .setOnClickListener(
                        (v) -> {
                            connectToNativeService("com.ferrochrome.rustbackend");
                        });

        findViewById(R.id.connect_to_rust_rpc)
                .setOnClickListener(
                        (v) -> {
                            connectToRpcService();
                        });

        findViewById(R.id.ping)
                .setOnClickListener(
                        (v) -> {
                            if (service == null) {
                                printLog("Not connected yet. Ignoring ping");
                                return;
                            }
                            try {
                                printLog("Ping to the service " + service.ping());

                                if (instance != null) {
                                    printLog("Ping to the instance " + instance.ping());
                                } else {
                                    printLog("Can't ping to instance");
                                }
                            } catch (Exception e) {
                                printLog("Failed to ping", e);
                            }
                        });
    }

    private boolean isConnected() {
        return serviceConnection != null || service != null || instance != null;
    }

    @MainThread
    private void handleConnect(IBinder svc) {
        try {
            service = IService.Stub.asInterface(svc);
            service.asBinder().linkToDeath(deathRecipientSvc, 0);

            instance = this.service.create();
            if (instance != null) {
                instance.asBinder().linkToDeath(deathRecipientInst, 0);
            } else {
                printLog("Service has no instance");
            }
        } catch (RemoteException e) {
            printLog("Failed to handle connect", e);
        }
    }

    @MainThread
    private void handleDisconnect() {
        if (!isConnected()) {
            printLog("..", new Exception());
            return;
        }
        // Note: unbindService(serviceConnection) is no longer required when disconnected.
        serviceConnection = null;

        // DISCLAIMER: Uncomment here for normal cases.
        /*
        if (service != null) {
            service.asBinder().unlinkToDeath(deathRecipientSvc, 0);
            service = null;
        }
        if (instance != null) {
            instance.asBinder().unlinkToDeath(deathRecipientInst, 0);
            instance = null;
        }
        */
    }

    private void printLog(String msg) {
        printLog(msg, null);
    }

    private void printLog(String msg, Exception e) {
        if (e != null) {
            Log.e(TAG, msg, e);
        } else {
            Log.e(TAG, msg);
        }
        logs += msg + "\n";
        logView.setText(logs);
    }

    private void connectToService(String pkg, String cls) {
        if (isConnected()) {
            printLog("Already connected. Ignoring...");
            return;
        }

        Intent intent = new Intent();
        intent.setClassName(pkg, cls);
        serviceConnection = new MyServiceConnection(this);
        if (!bindService(intent, serviceConnection, BIND_AUTO_CREATE)) {
            printLog("Failed to connect to " + pkg);
            serviceConnection = null;
        }
    }

    private void connectToNativeService(String tag) {
        if (isConnected()) {
            printLog("Already connected. Ignoring...");
            return;
        }

        printLog(
                "Waiting for native "
                        + tag
                        + ". Manually run /data/local/tmp/rustbackend if you haven't");
        IBinder service = ServiceManager.waitForService(tag);
        if (service == null) {
            printLog("Failed to find native service " + tag + ", which requires to run manually");
            return;
        }
        printLog("Connected to native " + tag);
        handleConnect(service);
    }

    private native IBinder nativeConnect();

    private void connectToRpcService() {
        if (isConnected()) {
            printLog("Already connected. Ignoring...");
            return;
        }

        printLog("Connecting to RPC service. I'll spawn myself");
        IBinder binder = nativeConnect();
        if (binder == null) {
            printLog("Failed to connect");
            return;
        }
        printLog("Connected to RPC service");
        handleConnect(binder);
    }

    @MainThread
    class MyServiceConnection implements ServiceConnection {
        private WeakReference<MainActivity> activity;

        MyServiceConnection(MainActivity activity) {
            this.activity = new WeakReference<MainActivity>(activity);
        }

        public void onServiceConnected(ComponentName name, IBinder service) {
            MainActivity activity = this.activity.get();
            if (activity == null || activity.serviceConnection == null) {
                // Ignore incoming connection or disconnection after activity is destroyed.
                return;
            }

            printLog("Connected to " + name);

            handleConnect(service);
        }

        public void onServiceDisconnected(ComponentName name) {
            MainActivity activity = this.activity.get();
            if (activity == null || activity.serviceConnection == null) {
                // Ignore incoming connection or disconnection after activity is destroyed.
                return;
            }

            printLog("Disconnected from " + name);
            activity.handleDisconnect();
        }
    }
}
