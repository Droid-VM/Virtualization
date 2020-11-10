package android.system.virtmanager;

interface IVirtManager {
        // Start the VM with the given ID, and return the CID allocated to it.
        int start_vm(String vm_id);
}
