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

import android.app.Activity;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.ServiceConnection;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.os.Bundle;
import android.os.IBinder;
import android.util.DisplayMetrics;
import android.util.Log;
import android.view.Display;
import android.view.InputDevice;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.view.WindowMetrics;

import java.util.ArrayList;

public class MainActivity extends Activity {

    private static final String TAG = "VmLauncherApp";

    private static final boolean DEBUG = true;

    VmRunnerService mVmRunnerService;

    Object mVmRunnerServiceReadyLock = new Object();
    boolean mVmRunnerServiceReady = false;

    // Actions that are deferred until the runner service is available and
    // a VM has started (e.g. setSurface()).
    ArrayList<Runnable> mDeferredRunnables = new ArrayList<Runnable>();

    /** Defines callbacks for service binding, passed to bindService(). */
    private ServiceConnection mServiceConnection =
            new ServiceConnection() {
                @Override
                public void onServiceConnected(ComponentName className, IBinder service) {
                    if (DEBUG) Log.v(TAG, "Connected to VmRunnerService");
                    // We've bound to VmRunnerService, cast the IBinder and get
                    // VmRunnerServiceBinder instance.
                    VmRunnerService.LocalBinder binder = (VmRunnerService.LocalBinder) service;

                    synchronized (mVmRunnerServiceReadyLock) {
                        mVmRunnerService = binder.getService();
                        mVmRunnerServiceReady = true;

                        startVm();

                        for (Runnable runnable : mDeferredRunnables) {
                            runnable.run();
                        }
                        mDeferredRunnables.clear();
                    }
                }

                @Override
                public void onServiceDisconnected(ComponentName arg) {
                    if (DEBUG) Log.v(TAG, "Disconnected from VmRunnerService");
                    mVmRunnerServiceReady = false;
                }
            };

    @Override
    public boolean onKeyUp(int keyCode, KeyEvent event) {
        if (!mVmRunnerServiceReady) {
            if (DEBUG) Log.v(TAG, "Failed to send " + event + " to VM: Service not connected");
            return false;
        }
        return mVmRunnerService.onKeyUp(event);
    }

    @Override
    public boolean onKeyDown(int keyCode, KeyEvent event) {
        if (!mVmRunnerServiceReady) {
            if (DEBUG) Log.v(TAG, "Failed to send " + event + " to VM: Service not connected");
            return false;
        }
        return mVmRunnerService.onKeyDown(event);
    }

    private boolean onTouch(MotionEvent event) {
        if (!mVmRunnerServiceReady) {
            if (DEBUG) Log.v(TAG, "Failed to send " + event + " to VM: Service not connected");
            return false;
        }
        return mVmRunnerService.onTouch(event);
    }

    private boolean onMouse(MotionEvent event) {
        if (!mVmRunnerServiceReady) {
            if (DEBUG) Log.v(TAG, "Failed to send " + event + " to VM: Service not connected");
            return false;
        }
        return mVmRunnerService.onMouse(event);
    }

    private void addVmDependentRunnable(Runnable runnable) {
        synchronized (mVmRunnerServiceReadyLock) {
            if (!mVmRunnerServiceReady) {
                mDeferredRunnables.add(runnable);
                return;
            }
        }
        runnable.run();
    }

    private boolean onScanoutSurfaceCreated(Surface surface) {
        if (DEBUG) Log.v(TAG, "onScanoutSurfaceCreated()");

        if (!mVmRunnerServiceReady) {
            if (DEBUG) Log.e(TAG, "Failed to set scanout surface: Service not connected");
            return false;
        }

        return mVmRunnerService.onScanoutSurfaceCreated(surface);
    }

    private boolean onScanoutSurfaceDestroyed() {
        if (DEBUG) Log.v(TAG, "onScanoutSurfaceDestroyed()");

        if (!mVmRunnerServiceReady) {
            if (DEBUG) Log.e(TAG, "Failed to remove scanout surface: Service not connected");
            return false;
        }

        return mVmRunnerService.onScanoutSurfaceDestroyed();
    }

    private boolean onCursorSurfaceCreated(SurfaceView surfaceView, Surface surface) {
        if (DEBUG) Log.v(TAG, "onCursorSurfaceCreated()");

        if (!mVmRunnerServiceReady) {
            if (DEBUG) Log.e(TAG, "Failed to set cursor surface: Service not connected");
            return false;
        }

        return mVmRunnerService.onCursorSurfaceCreated(surfaceView, surface);
    }

    private boolean onCursorSurfaceDestroyed() {
        if (DEBUG) Log.v(TAG, "onCursorSurfaceDestroyed()");

        if (!mVmRunnerServiceReady) {
            if (DEBUG) Log.e(TAG, "Failed to remove cursor surface: Service not connected");
            return false;
        }

        return mVmRunnerService.onCursorSurfaceDestroyed();
    }

    @Override
    public void onPause() {
        if (DEBUG) Log.v(TAG, "onPause()");
        super.onResume();
    }

    @Override
    public void onResume() {
        if (DEBUG) Log.v(TAG, "onResume()");
        super.onResume();
    }

    @Override
    public void onRestart() {
        if (DEBUG) Log.v(TAG, "onRestart()");
        super.onRestart();
    }

    @Override
    public void onStart() {
        if (DEBUG) Log.v(TAG, "onStart()");
        super.onStart();

        if (DEBUG) Log.v(TAG, "Connecting to VmRunnerService");
        Intent intent = new Intent(this, VmRunnerService.class);
        bindService(intent, mServiceConnection, Context.BIND_AUTO_CREATE);
    }

    @Override
    public void onStop() {
        if (DEBUG) Log.v(TAG, "onStop()");
        super.onStop();

        if (DEBUG) Log.v(TAG, "Disconnecting from VmRunnerService");
        unbindService(mServiceConnection);
        mVmRunnerServiceReady = false;
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        if (DEBUG) Log.v(TAG, "onCreate()");
        super.onCreate(savedInstanceState);

        if (DEBUG) Log.v(TAG, "Starting VmRunnerService");
        startService(new Intent(this, VmRunnerService.class));

        getWindow().setDecorFitsSystemWindows(false);
        setContentView(R.layout.activity_main);

        View backgroundTouchView = findViewById(R.id.background_touch_view);
        backgroundTouchView.setOnTouchListener(
                (v, event) -> {
                    return onTouch(event);
                });

        SurfaceView surfaceView = findViewById(R.id.surface_view);
        surfaceView.requestUnbufferedDispatch(InputDevice.SOURCE_ANY);
        surfaceView.setOnCapturedPointerListener(
                (v, event) -> {
                    return onMouse(event);
                });
        surfaceView
                .getHolder()
                .addCallback(
                        // TODO(b/331708504): it should be handled in AVF framework.
                        new SurfaceHolder.Callback() {
                            @Override
                            public void surfaceCreated(SurfaceHolder holder) {
                                if (DEBUG) Log.v(TAG, "Scanout surfaceCreated()");
                                addVmDependentRunnable(
                                        () -> {
                                            onScanoutSurfaceCreated(holder.getSurface());
                                        });
                            }

                            @Override
                            public void surfaceChanged(
                                    SurfaceHolder holder, int format, int width, int height) {
                                Log.d(TAG, "width: " + width + ", height: " + height);
                            }

                            @Override
                            public void surfaceDestroyed(SurfaceHolder holder) {
                                if (DEBUG) Log.v(TAG, "Scanout surfaceDestroyed()");
                                onScanoutSurfaceDestroyed();
                            }
                        });

        SurfaceView cursorSurfaceView = findViewById(R.id.cursor_surface_view);
        cursorSurfaceView.setZOrderMediaOverlay(true);
        cursorSurfaceView.getHolder().setFormat(PixelFormat.RGBA_8888);
        cursorSurfaceView
                .getHolder()
                .addCallback(
                        new SurfaceHolder.Callback() {
                            @Override
                            public void surfaceCreated(SurfaceHolder holder) {
                                if (DEBUG) Log.v(TAG, "Scanout surfaceCreated()");
                                addVmDependentRunnable(
                                        () -> {
                                            onCursorSurfaceCreated(
                                                    cursorSurfaceView, holder.getSurface());
                                        });
                            }

                            @Override
                            public void surfaceChanged(
                                    SurfaceHolder holder, int format, int width, int height) {
                                Log.d(TAG, "width: " + width + ", height: " + height);
                            }

                            @Override
                            public void surfaceDestroyed(SurfaceHolder holder) {
                                if (DEBUG) Log.v(TAG, "Cursor surfaceDestroyed()");
                                onScanoutSurfaceDestroyed();
                            }
                        });

        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);

        // Fullscreen:
        WindowInsetsController windowInsetsController = surfaceView.getWindowInsetsController();
        windowInsetsController.setSystemBarsBehavior(
                WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
        windowInsetsController.hide(WindowInsets.Type.systemBars());
    }

    @Override
    public void onDestroy() {
        if (DEBUG) Log.v(TAG, "onDestroy()");
        super.onDestroy();

        stopService(new Intent(this, VmRunnerService.class));
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            SurfaceView surfaceView = findViewById(R.id.surface_view);
            Log.d(TAG, "requestPointerCapture()");
            surfaceView.requestPointerCapture();
        }
    }

    private void startVm() {
        if (DEBUG) Log.v(TAG, "startVm()");

        WindowMetrics windowMetrics = getWindowManager().getCurrentWindowMetrics();
        Rect windowSize = windowMetrics.getBounds();
        int width = windowSize.right;
        int height = windowSize.bottom;
        int dpi = (int) (DisplayMetrics.DENSITY_DEFAULT * windowMetrics.getDensity());
        int rr = 30;
        Display display = getDisplay();
        if (display != null) {
            rr = (int) display.getRefreshRate();
        }

        if (!mVmRunnerService.startVm(width, height, dpi, rr)) {
            Log.e(TAG, "Failed to start VM.");
            return;
        }
    }

}
