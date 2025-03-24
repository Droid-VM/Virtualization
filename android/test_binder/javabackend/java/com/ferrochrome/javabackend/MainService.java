package com.ferrochrome.javabackend;

import android.app.Service;
import android.content.ComponentName;
import android.content.Intent;
import android.content.ServiceConnection;
import android.os.IBinder;
import android.util.Log;

import androidx.annotation.Nullable;

import com.ferrochrome.IInstance;
import com.ferrochrome.IService;

public class MainService extends Service {
    private static final String TAG = "JavaBackend";
    private final Object lock = new Object();
    private IInstance instance;

    public void onCreate() {
        super.onCreate();
        Log.i(TAG, "main: created...");

        // We don't need complex logic here because we wouldn't unbind here.
        Intent intent = new Intent(this, InstanceService.class);
        ServiceConnection conn =
                new ServiceConnection() {
                    public void onServiceConnected(ComponentName name, IBinder service) {
                        Log.i(TAG, "main: connected to sub instance service");
                        synchronized (lock) {
                            instance = IInstance.Stub.asInterface(service);
                            lock.notify();
                        }
                    }

                    public void onServiceDisconnected(ComponentName name) {
                        Log.e(TAG, "main: disconnected wtf?");
                    }
                };
        bindService(intent, conn, BIND_AUTO_CREATE);
    }

    @Nullable
    @Override
    public IBinder onBind(Intent intent) {
        return new IService.Stub() {
            @Override
            public boolean ping() {
                Log.i(TAG, "main: ping");
                return true;
            }

            @Override
            public IInstance create() {
                Log.i(TAG, "main: returning instance");
                synchronized (lock) {
                    while (instance == null) {
                        try {
                            lock.wait();
                        } catch (Exception e) {
                            // ignore..
                        }
                    }
                    return instance;
                }
            }
        };
    }
}
