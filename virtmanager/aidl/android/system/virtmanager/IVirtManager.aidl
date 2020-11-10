package android.system.virtmanager;

interface IVirtManager {
        // Start the VM with the given ID, and return the CID allocated to it.
        int start_vm(String vm_id);

        // Decrement the refcount of the VM with the given ID, if a reference is held by the calling
        // process. If the refcount reaches 0 then the VM may be shut down.
        void drop_vm_reference(String vm_id);
}
