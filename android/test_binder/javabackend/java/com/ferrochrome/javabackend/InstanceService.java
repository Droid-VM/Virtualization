package com.ferrochrome.javabackend;

import android.app.Service;
import android.content.Intent;
import android.os.IBinder;
import android.util.Log;

import androidx.annotation.Nullable;

import com.ferrochrome.IInstance;

public class InstanceService extends Service {
    private static final String TAG = "JavaBackend";

    public void onCreate() {
        super.onCreate();
        Log.i(TAG, "instance: created...");
    }

    @Nullable
    @Override
    public IBinder onBind(Intent intent) {
        return new IInstance.Stub() {
            @Override
            public boolean ping() {
                Log.i(TAG, "instance: ping from instance");
                return true;
            }
        };
    }
}
