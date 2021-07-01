/*
 * Copyright (C) 2021 The Android Open Source Project
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

import android.content.Context;

import java.lang.ref.WeakReference;
import java.util.Map;
import java.util.WeakHashMap;

/**
 * Manages {@link VirtualMachine} objects created for an application.
 *
 * @hide
 */
public class VirtualMachineManager {
    private final Context mContext;

    private VirtualMachineManager(Context context) {
        mContext = context;
    }

    static Map<Context, WeakReference<VirtualMachineManager>> sInstances = new WeakHashMap<>();

    /** Returns the per-context instance. */
    public static VirtualMachineManager getInstance(Context context) {
        synchronized (sInstances) {
            VirtualMachineManager vmm =
                    sInstances.containsKey(context) ? sInstances.get(context).get() : null;
            if (vmm == null) {
                vmm = new VirtualMachineManager(context);
                sInstances.put(context, new WeakReference(vmm));
            }
            return vmm;
        }
    }

    /**
     * Creates a new {@link VirtualMachine}. Every call to this creates a new (and different)
     * virtual machine even if the name and the config are the same.
     */
    public VirtualMachine create(String name, VirtualMachineConfig config)
            throws VirtualMachineException {
        return VirtualMachine.create(mContext, name, config);
    }

    /** Returns an existing {@link VirtualMachine} with the given name. */
    public VirtualMachine get(String name) throws VirtualMachineException {
        return VirtualMachine.load(mContext, name);
    }

    private static final Object sNameLock = new Object();

    /** Returns an existing {@link VirtualMachine} if it exists, or create a new one. */
    public VirtualMachine getOrCreate(String name, VirtualMachineConfig config)
            throws VirtualMachineException {
        VirtualMachine vm;
        synchronized (sNameLock) {
            vm = get(name);
            if (vm == null) {
                return create(name, config);
            }
        }

        if (vm.getConfig().equals(config)) {
            return vm;
        } else {
            throw new VirtualMachineException("Incompatible config");
        }
    }
}
