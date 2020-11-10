package android.system.virtmanager;

import android.system.virtmanager.IVirtualMachine;

interface IVirtManager {
        // Start the VM with the given ID, and return the CID allocated to it.
        IVirtualMachine start_vm(String config_path);
}
