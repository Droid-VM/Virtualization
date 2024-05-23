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

import static android.system.virtualmachine.VirtualMachineConfig.CPU_TOPOLOGY_MATCH_HOST;

import android.app.Service;
import android.content.Intent;
import android.crosvm.ICrosvmAndroidDisplayService;
import android.os.Binder;
import android.os.IBinder;
import android.os.ParcelFileDescriptor;
import android.os.RemoteException;
import android.os.ServiceManager;
import android.system.virtualizationservice_internal.IVirtualizationServiceInternal;
import android.system.virtualmachine.VirtualMachine;
import android.system.virtualmachine.VirtualMachineCallback;
import android.system.virtualmachine.VirtualMachineConfig;
import android.system.virtualmachine.VirtualMachineCustomImageConfig;
import android.system.virtualmachine.VirtualMachineCustomImageConfig.DisplayConfig;
import android.system.virtualmachine.VirtualMachineCustomImageConfig.GpuConfig;
import android.system.virtualmachine.VirtualMachineException;
import android.system.virtualmachine.VirtualMachineManager;
import android.util.Log;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.Surface;
import android.view.SurfaceView;

import libcore.io.IoBridge;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.BufferedOutputStream;
import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * The persistent service that manages the VM for this application that continues running when the
 * application is stopped (in the background).
 */
public class VmRunnerService extends Service {

    public static final String TAG = "VmRunnerService";

    private static final String VM_NAME = "my_custom_vm";

    private static final boolean DEBUG = true;

    // Binder given to clients (currently just MainActivity).
    private final IBinder mBinder = new LocalBinder();

    // If not null, the running VirtualMachine.
    private VirtualMachine mVirtualMachine;

    private ParcelFileDescriptor mCursorStream;

    private final ExecutorService mExecutorService = Executors.newFixedThreadPool(4);

    public class LocalBinder extends Binder {
        VmRunnerService getService() {
            return VmRunnerService.this;
        }
    }

    @Override
    public IBinder onBind(Intent intent) {
        if (DEBUG) Log.v(TAG, "onBind()");
        return mBinder;
    }

    private VirtualMachineConfig createVirtualMachineConfig(
            int width, int height, int dpi, int rr, String jsonPath) {
        VirtualMachineConfig.Builder configBuilder =
                new VirtualMachineConfig.Builder(getApplication());
        configBuilder.setCpuTopology(CPU_TOPOLOGY_MATCH_HOST);

        configBuilder.setProtectedVm(false);
        if (DEBUG) {
            configBuilder.setDebugLevel(VirtualMachineConfig.DEBUG_LEVEL_FULL);
            configBuilder.setVmOutputCaptured(true);
        }
        VirtualMachineCustomImageConfig.Builder customImageConfigBuilder =
                new VirtualMachineCustomImageConfig.Builder();
        try {
            String rawJson = new String(Files.readAllBytes(Path.of(jsonPath)));
            JSONObject json = new JSONObject(rawJson);
            customImageConfigBuilder.setName(json.optString("name", ""));
            if (json.has("kernel")) {
                customImageConfigBuilder.setKernelPath(json.getString("kernel"));
            }
            if (json.has("initrd")) {
                customImageConfigBuilder.setInitrdPath(json.getString("initrd"));
            }
            if (json.has("params")) {
                Arrays.stream(json.getString("params").split(" "))
                        .forEach(customImageConfigBuilder::addParam);
            }
            if (json.has("bootloader")) {
                customImageConfigBuilder.setBootloaderPath(json.getString("bootloader"));
            }
            if (json.has("disks")) {
                JSONArray diskArr = json.getJSONArray("disks");
                for (int i = 0; i < diskArr.length(); i++) {
                    JSONObject item = diskArr.getJSONObject(i);
                    if (item.has("image")) {
                        if (item.optBoolean("writable", false)) {
                            customImageConfigBuilder.addDisk(
                                    VirtualMachineCustomImageConfig.Disk.RWDisk(
                                            item.getString("image")));
                        } else {
                            customImageConfigBuilder.addDisk(
                                    VirtualMachineCustomImageConfig.Disk.RODisk(
                                            item.getString("image")));
                        }
                    }
                }
            }
            if (json.has("console_input_device")) {
                configBuilder.setConsoleInputDevice(json.getString("console_input_device"));
            }
            if (json.has("gpu")) {
                JSONObject gpuJson = json.getJSONObject("gpu");

                GpuConfig.Builder gpuConfigBuilder = new GpuConfig.Builder();

                if (gpuJson.has("backend")) {
                    gpuConfigBuilder.setBackend(gpuJson.getString("backend"));
                }
                if (gpuJson.has("context_types")) {
                    ArrayList<String> contextTypes = new ArrayList<String>();
                    JSONArray contextTypesJson = gpuJson.getJSONArray("context_types");
                    for (int i = 0; i < contextTypesJson.length(); i++) {
                        contextTypes.add(contextTypesJson.getString(i));
                    }
                    gpuConfigBuilder.setContextTypes(contextTypes.toArray(new String[0]));
                }
                if (gpuJson.has("pci_address")) {
                    gpuConfigBuilder.setPciAddress(gpuJson.getString("pci_address"));
                }
                if (gpuJson.has("renderer_features")) {
                    gpuConfigBuilder.setRendererFeatures(gpuJson.getString("renderer_features"));
                }
                if (gpuJson.has("renderer_use_egl")) {
                    gpuConfigBuilder.setRendererUseEgl(gpuJson.getBoolean("renderer_use_egl"));
                }
                if (gpuJson.has("renderer_use_gles")) {
                    gpuConfigBuilder.setRendererUseGles(gpuJson.getBoolean("renderer_use_gles"));
                }
                if (gpuJson.has("renderer_use_glx")) {
                    gpuConfigBuilder.setRendererUseGlx(gpuJson.getBoolean("renderer_use_glx"));
                }
                if (gpuJson.has("renderer_use_surfaceless")) {
                    gpuConfigBuilder.setRendererUseSurfaceless(
                            gpuJson.getBoolean("renderer_use_surfaceless"));
                }
                if (gpuJson.has("renderer_use_vulkan")) {
                    gpuConfigBuilder.setRendererUseVulkan(
                            gpuJson.getBoolean("renderer_use_vulkan"));
                }
                customImageConfigBuilder.setGpuConfig(gpuConfigBuilder.build());
            }

            configBuilder.setMemoryBytes(8L * 1024 * 1024 * 1024 /* 8 GB */);

            DisplayConfig.Builder displayConfigBuilder = new DisplayConfig.Builder();
            displayConfigBuilder.setWidth(width);
            displayConfigBuilder.setHeight(height);
            displayConfigBuilder.setHorizontalDpi(dpi);
            displayConfigBuilder.setVerticalDpi(dpi);
            displayConfigBuilder.setRefreshRate(rr);
            customImageConfigBuilder.setDisplayConfig(displayConfigBuilder.build());

            customImageConfigBuilder.useTouch(true);
            customImageConfigBuilder.useKeyboard(true);
            customImageConfigBuilder.useMouse(true);

            configBuilder.setCustomImageConfig(customImageConfigBuilder.build());
        } catch (JSONException | IOException e) {
            throw new IllegalStateException("malformed input", e);
        }
        return configBuilder.build();
    }

    public boolean startVm(int width, int height, int dpi, int rr) {
        if (DEBUG) Log.v(TAG, "startVm()");

        if (mVirtualMachine != null) {
            if (DEBUG) Log.v(TAG, "VM already running.");
            return true;
        }

        if (DEBUG) Log.v(TAG, "VM not yet running. Attempting to start VM.");

        try {
            // To ensure that the previous display service is removed.
            IVirtualizationServiceInternal.Stub.asInterface(
                            ServiceManager.waitForService("android.system.virtualizationservice"))
                    .clearDisplayService();
        } catch (RemoteException e) {
            Log.d(TAG, "failed to clearDisplayService");
        }

        VirtualMachineCallback callback =
                new VirtualMachineCallback() {
                    // store reference to ExecutorService to avoid race condition
                    private final ExecutorService mService = mExecutorService;

                    @Override
                    public void onPayloadStarted(VirtualMachine vm) {
                        Log.e(TAG, "payload start");
                    }

                    @Override
                    public void onPayloadReady(VirtualMachine vm) {
                        // This check doesn't 100% prevent race condition or UI hang.
                        // However, it's fine for demo.
                        if (mService.isShutdown()) {
                            return;
                        }
                        Log.d(TAG, "(Payload is ready. Testing VM service...)");
                    }

                    @Override
                    public void onPayloadFinished(VirtualMachine vm, int exitCode) {
                        // This check doesn't 100% prevent race condition, but is fine for demo.
                        if (!mService.isShutdown()) {
                            Log.d(
                                    TAG,
                                    String.format("(Payload finished. exit code: %d)", exitCode));
                        }
                    }

                    @Override
                    public void onError(VirtualMachine vm, int errorCode, String message) {
                        Log.d(
                                TAG,
                                String.format(
                                        "(Error occurred. code: %d, message: %s)",
                                        errorCode, message));
                    }

                    @Override
                    public void onStopped(VirtualMachine vm, int reason) {
                        Log.e(TAG, "vm stop");
                    }
                };

        try {
            VirtualMachineConfig config =
                    createVirtualMachineConfig(
                            width, height, dpi, rr, "/data/local/tmp/vm_config.json");
            VirtualMachineManager vmm =
                    getApplication().getSystemService(VirtualMachineManager.class);
            if (vmm == null) {
                Log.e(TAG, "vmm is null");
                return false;
            }
            mVirtualMachine = vmm.getOrCreate(VM_NAME, config);
            try {
                mVirtualMachine.setConfig(config);
            } catch (VirtualMachineException e) {
                vmm.delete(VM_NAME);
                mVirtualMachine = vmm.create(VM_NAME, config);
                Log.e(TAG, "error" + e);
            }

            Log.d(TAG, "vm start");
            mVirtualMachine.run();
            mVirtualMachine.setCallback(Executors.newSingleThreadExecutor(), callback);
            if (DEBUG) {
                InputStream console = mVirtualMachine.getConsoleOutput();
                InputStream log = mVirtualMachine.getLogOutput();
                OutputStream consoleLogFile =
                        new LineBufferedOutputStream(
                                getApplicationContext().openFileOutput("console.log", 0));
                mExecutorService.execute(new CopyStreamTask("console", console, consoleLogFile));
                mExecutorService.execute(new Reader("log", log));
            }
        } catch (VirtualMachineException | IOException e) {
            throw new RuntimeException(e);
        }

        Log.d(TAG, "VM started!");
        return true;
    }

    public boolean onKeyDown(KeyEvent event) {
        if (mVirtualMachine == null) {
            if (DEBUG) Log.v(TAG, "VM not yet running. Dropping event " + event);
            return false;
        }
        return mVirtualMachine.sendKeyEvent(event);
    }

    public boolean onKeyUp(KeyEvent event) {
        if (mVirtualMachine == null) {
            if (DEBUG) Log.v(TAG, "VM not yet running. Dropping event " + event);
            return false;
        }
        return mVirtualMachine.sendKeyEvent(event);
    }

    public boolean onTouch(MotionEvent event) {
        if (mVirtualMachine == null) {
            if (DEBUG) Log.v(TAG, "VM not yet running. Dropping event " + event);
            return false;
        }
        return mVirtualMachine.sendSingleTouchEvent(event);
    }

    public boolean onMouse(MotionEvent event) {
        if (mVirtualMachine == null) {
            if (DEBUG) Log.v(TAG, "VM not yet running. Dropping event " + event);
            return false;
        }
        return mVirtualMachine.sendMouseEvent(event);
    }

    public boolean onScanoutSurfaceCreated(Surface surface) {
        if (DEBUG) Log.v(TAG, "onScanoutSurfaceCreated()");

        if (mVirtualMachine == null) {
            if (DEBUG) Log.v(TAG, "VM not yet running. Dropping surface " + surface);
            return false;
        }

        if (DEBUG) Log.v(TAG, "Sending surface to VM");
        runWithDisplayService((service) -> service.setSurface(surface, false /* forCursor */));

        return true;
    }

    public boolean onScanoutSurfaceDestroyed() {
        if (DEBUG) Log.v(TAG, "onScanoutSurfaceDestroyed()");

        if (mVirtualMachine == null) {
            if (DEBUG) Log.v(TAG, "VM not yet running. Dropping surface removal");
            return false;
        }

        if (DEBUG) Log.v(TAG, "Sending surface removal request to VM");
        runWithDisplayService((service) -> service.removeSurface(false /* forCursor */));

        return true;
    }

    public boolean onCursorSurfaceCreated(SurfaceView surfaceView, Surface surface) {
        if (DEBUG) Log.v(TAG, "onCursorSurfaceCreated()");

        if (mVirtualMachine == null) {
            if (DEBUG) Log.v(TAG, "VM not yet running. Dropping surface removal");
            return false;
        }

        try {
            ParcelFileDescriptor[] pfds = ParcelFileDescriptor.createSocketPair();
            mExecutorService.execute(new CursorHandler(surfaceView, pfds[0]));
            mCursorStream = pfds[0];
            runWithDisplayService((service) -> service.setCursorStream(pfds[1]));
        } catch (Exception e) {
            Log.e("TAG", "Failed to run cursor stream handler", e);
        }

        if (DEBUG) Log.v(TAG, "Sending cursor surface to VM");
        runWithDisplayService((service) -> service.setSurface(surface, true /* forCursor */));

        return true;
    }

    public boolean onCursorSurfaceDestroyed() {
        if (DEBUG) Log.v(TAG, "onCursorSurfaceDestroyed()");

        if (mVirtualMachine == null) {
            if (DEBUG) Log.v(TAG, "VM not yet running. Dropping surface removal");
            return false;
        }

        if (DEBUG) Log.v(TAG, "Sending cursor surface removal request to VM");
        runWithDisplayService((service) -> service.removeSurface(true /* forCursor */));

        if (mCursorStream != null) {
            try {
                if (DEBUG) Log.v(TAG, "Closing cursor stream");
                mCursorStream.close();
            } catch (IOException e) {
                Log.d(TAG, "failed to close fd", e);
            }
        }

        return true;
    }

    @Override
    public void onCreate() {
        if (DEBUG) Log.v(TAG, "onCreate");
        super.onCreate();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (DEBUG) Log.v(TAG, "onStartCommand()");
        return START_STICKY;
    }

    @Override
    public void onDestroy() {
        if (DEBUG) Log.v(TAG, "onDestroy()");
        super.onDestroy();
    }

    @FunctionalInterface
    public interface RemoteExceptionCheckedFunction<T> {
        void apply(T t) throws RemoteException;
    }

    private void runWithDisplayService(
            RemoteExceptionCheckedFunction<ICrosvmAndroidDisplayService> func) {
        IVirtualizationServiceInternal vs =
                IVirtualizationServiceInternal.Stub.asInterface(
                        ServiceManager.waitForService("android.system.virtualizationservice"));
        try {
            Log.d(TAG, "wait for the display service");
            ICrosvmAndroidDisplayService service =
                    ICrosvmAndroidDisplayService.Stub.asInterface(vs.waitDisplayService());
            assert service != null;
            func.apply(service);
            Log.d(TAG, "job done");
        } catch (Exception e) {
            Log.d(TAG, "error", e);
        }
    }

    /** Reads data from an input stream and posts it to the output data */
    static class Reader implements Runnable {
        private final String mName;
        private final InputStream mStream;

        Reader(String name, InputStream stream) {
            mName = name;
            mStream = stream;
        }

        @Override
        public void run() {
            try {
                BufferedReader reader = new BufferedReader(new InputStreamReader(mStream));
                String line;
                while ((line = reader.readLine()) != null && !Thread.interrupted()) {
                    Log.d(TAG, mName + ": " + line);
                }
            } catch (IOException e) {
                Log.e(TAG, "Exception while posting " + mName + " output: " + e.getMessage());
            }
        }
    }

    static class CursorHandler implements Runnable {
        private final SurfaceView mSurfaceView;
        private final ParcelFileDescriptor mStream;

        CursorHandler(SurfaceView s, ParcelFileDescriptor stream) {
            mSurfaceView = s;
            mStream = stream;
        }

        @Override
        public void run() {
            Log.d(TAG, "CursorHandler");
            try {
                ByteBuffer byteBuffer = ByteBuffer.allocate(8 /* (x: u32, y: u32) */);
                byteBuffer.order(ByteOrder.LITTLE_ENDIAN);
                while (true) {
                    byteBuffer.clear();
                    IoBridge.read(
                            mStream.getFileDescriptor(),
                            byteBuffer.array(),
                            0,
                            byteBuffer.array().length);
                    float x = (float) (byteBuffer.getInt() & 0xFFFFFFFF);
                    float y = (float) (byteBuffer.getInt() & 0xFFFFFFFF);
                    mSurfaceView.post(
                            () -> {
                                mSurfaceView.setTranslationX(x);
                                mSurfaceView.setTranslationY(y);
                            });
                }
            } catch (IOException e) {
                Log.e(TAG, e.getMessage());
            }
        }
    }

    private static class CopyStreamTask implements Runnable {
        private final String mName;
        private final InputStream mIn;
        private final OutputStream mOut;

        CopyStreamTask(String name, InputStream in, OutputStream out) {
            mName = name;
            mIn = in;
            mOut = out;
        }

        @Override
        public void run() {
            try {
                byte[] buffer = new byte[2048];
                while (!Thread.interrupted()) {
                    int len = mIn.read(buffer);
                    if (len < 0) {
                        break;
                    }
                    mOut.write(buffer, 0, len);
                }
            } catch (Exception e) {
                Log.e(TAG, "Exception while posting " + mName, e);
            }
        }
    }

    private static class LineBufferedOutputStream extends BufferedOutputStream {
        LineBufferedOutputStream(OutputStream out) {
            super(out);
        }

        @Override
        public void write(byte[] buf, int off, int len) throws IOException {
            super.write(buf, off, len);
            for (int i = 0; i < len; ++i) {
                if (buf[off + i] == '\n') {
                    flush();
                    break;
                }
            }
        }
    }
}
