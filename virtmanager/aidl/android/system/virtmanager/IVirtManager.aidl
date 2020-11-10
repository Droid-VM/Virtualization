package android.system.virtmanager;

import android.system.virtmanager.IVirtualMachine;

interface IVirtManager {
        /** Start the VM with the given config file, and return a handle to it. */
        IVirtualMachine start_vm(in String config_path);
}
