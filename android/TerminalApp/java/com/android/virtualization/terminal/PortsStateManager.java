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

import android.content.Context;
import android.content.SharedPreferences;

import com.android.internal.annotations.GuardedBy;

import java.util.HashSet;
import java.util.Set;
import java.util.stream.Collectors;

/**
 * PortsStateManager is responsible for communicating with shared preferences and managing state of
 * ports.
 */
public class PortsStateManager {
    private static final String PREFS_NAME = ".PORTS";
    private static final int FLAG_ENABLED = 1;

    private static PortsStateManager mInstance;
    private final Object mLock = new Object();

    private final SharedPreferences mSharedPref;

    @GuardedBy("mLock")
    private Set<Integer> mActivePorts;

    @GuardedBy("mLock")
    private final Set<Integer> mEnabledPorts;

    @GuardedBy("mLock")
    private final Set<Listener> mListeners;

    private PortsStateManager(SharedPreferences sharedPref) {
        mSharedPref = sharedPref;
        mEnabledPorts =
                mSharedPref.getAll().entrySet().stream()
                        .filter(entry -> entry.getValue() instanceof Integer)
                        .filter(entry -> ((int) entry.getValue() & FLAG_ENABLED) == FLAG_ENABLED)
                        .map(entry -> entry.getKey())
                        .filter(
                                key -> {
                                    try {
                                        Integer.parseInt(key);
                                        return true;
                                    } catch (NumberFormatException e) {
                                        return false;
                                    }
                                })
                        .map(Integer::parseInt)
                        .collect(Collectors.toSet());
        mActivePorts = new HashSet<>();
        mListeners = new HashSet<>();
    }

    static PortsStateManager getInstance(Context context) {
        if (mInstance == null) {
            SharedPreferences sharedPref =
                    context.getSharedPreferences(
                            context.getPackageName() + PREFS_NAME, Context.MODE_PRIVATE);
            synchronized (PortsStateManager.class) {
                if (mInstance == null) {
                    mInstance = new PortsStateManager(sharedPref);
                }
            }
        }
        return mInstance;
    }

    Set<Integer> getActivePorts() {
        return mActivePorts;
    }

    Set<Integer> getEnabledPorts() {
        return mEnabledPorts;
    }

    void updateActivePorts(Set<Integer> ports) {
        synchronized (mLock) {
            notifyActivePortsUpdated(mActivePorts, ports);
            mActivePorts = ports;
        }
    }

    void enablePort(int port) {
        synchronized (mLock) {
            SharedPreferences.Editor editor = mSharedPref.edit();
            editor.putInt(String.valueOf(port), FLAG_ENABLED);
            editor.apply();
            mEnabledPorts.add(port);
            notifyPortEnabled(port);
        }
    }

    void disablePort(int port) {
        synchronized (mLock) {
            SharedPreferences.Editor editor = mSharedPref.edit();
            editor.putInt(String.valueOf(port), 0);
            editor.apply();
            mEnabledPorts.remove(port);
            notifyPortDisabled(port);
        }
    }

    void registerListener(Listener listener) {
        synchronized (mLock) {
            mListeners.add(listener);
        }
    }

    void unregisterListener(Listener listener) {
        synchronized (mLock) {
            mListeners.remove(listener);
        }
    }

    private void notifyActivePortsUpdated(Set<Integer> oldPorts, Set<Integer> newPorts) {
        for (Listener listener : mListeners) {
            listener.onActivePortsUpdated(oldPorts, newPorts);
        }
    }

    private void notifyPortEnabled(int port) {
        for (Listener listener : mListeners) {
            listener.onPortEnabled(port);
        }
    }

    private void notifyPortDisabled(int port) {
        for (Listener listener : mListeners) {
            listener.onPortDisabled(port);
        }
    }

    public abstract static class Listener {
        void onActivePortsUpdated(Set<Integer> oldPorts, Set<Integer> newPorts) {}

        void onPortEnabled(int port) {}

        void onPortDisabled(int port) {}
    }
}
