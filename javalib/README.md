# Android Virtualization Framework API

These Java APIs allow an app to configure and run a Virtual Machine running
[Microdroid](../microdroid/README.md) and execute native code from the app (the
payload) within it.

There is more information on AVF [here](../README.md). To see how to package the
payload code that is to run inside a VM, and the API available to it, see the
[VM Payload API](../vm_payload/README.md)

Note that these APIs are all @SystemApi and require the restricted
`android.permission.MANAGE_VIRTUAL_MACHINE` permission, so they are not
available to third party apps.

## Detecting AVF Support

The simplest way to detect whether a device has support for AVF is to retrieve
an instance of the
[`VirtualMachineManager`](src/android/system/virtualmachine/VirtualMachineManager.java)
class; if the result is not `null` then the device has support.

If the result is not `null` you can then find out whether protected,
non-protected VMs, or both are supported using
`VirtualMachineManager#getCapabilities()`.

```Java
VirtualMachineManager vmm = context.getSystemService(VirtualMachineManager.class);
if (vmm == null) {
    // AVF is not supported.
} else {
    // AVF is supported.
    int capabilities = vmm.getCapabilities();
    if ((capabilties & CAPABILITY_PROTECTED_VM) != 0) {
        // Protected VMs supported.
    }
    if ((capabilties & CAPABILITY_NON_PROTECTED_VM) != 0) {
        // Non-Protected VMs supported.
    }
}
```

An alternative for detecting AVF support is to query support for the
`android.software.virtualization_framework` system feature:

```Java
if (getPackageManager().hasSystemFeature(PackageManager.FEATURE_VIRTUALIZATION_FRAMEWORK)) {
    // AVF is supported.
}
```

You can also express a dependency on this system feature in your app's manifest
with a
[`<uses-feature>`](https://developer.android.com/guide/topics/manifest/uses-feature-element)
element.


## Starting a VM

Once you have an instance of the
[`VirtualMachineManager`](src/android/system/virtualmachine/VirtualMachineManager.java),
a VM can be started by:
- Specifying the desired VM configuration, using a
  [`VirtualMachineConfig`](src/android/system/virtualmachine/VirtualMachineConfig.java)
  builder;
- Creating a new
  [`VirtualMachine`](src/android/system/virtualmachine/VirtualMachine.java)
  instance (or retrieving an existing one);
- Registering to retrieve events from the VM by providing a
  [`VirtualMachineCallback`](src/android/system/virtualmachine/VirtualMachineCallback.java);
- Running the VM.

A minimal example might look like this:

```Java
VirtualMachineConfig config =
        new VirtualMachineConfig.Builder(this)
            .setProtectedVm(true)
            .setPayloadBinaryName("my_payload.so")
            .build();

VirtualMachine vm = vmm.getOrCreate("my vm", config);

vm.setCallback(executor,
        new VirtualMachineCallback() {...});

vm.run();

```

Here we are running a protected VM, which will execute the code in the
`my_payload.so` file included in your APK.

Information about the VM, including its configuration, is stored in files in
your app's private data directory. Once an instance of a VM has been created it
can be retrieved by name even if the app is restarted or the device is
rebooted. Directly inspecting or modifying these files is not recommended.

The `getOrCreate` call will retrieve an existing VM instance if it exists (in
which case the `config` parameter is ignored), or create a new one
otherwise. There are also separate `get` and `create` methods.

The `run()` method is asynchronous; it returns successfully once the VM is
starting. You can find out when the VM is ready, or if it fails, via your
`VirtualMachineCallback` implementation.

## VM Configuration

There are other things that you can specify as part of the
[`VirtualMachineConfig`](src/android/system/virtualmachine/VirtualMachineConfig.java):
- Whether the VM should be debuggable. This is not secure, but it does allow
  access to logs from inside the VM, which can be useful for troubleshooting.
- How much memory should be allocated to the VM. (This is an upper bound;
  typically memory is allocated to the VM as it is needed until the limit is
  reached - but there is some overhead proportional to the maximum size.)
- How many virtual CPUs the VM has.
- How much encrypted storage the VM has.
- The path to the installed APK containing the code to run as the VM payload.

## VM Life-cycle

To find out the progress of the Virtual Machine once it is started you should
implement the methods defined by
[`VirtualMachineCallback`](src/android/system/virtualmachine/VirtualMachineCallback.java). These
are called when the following events happen:
- `onPayloadStarted()`: The VM payload is being run.
- `onPayloadReady()`: The VM payload is running and ready to accept
  connections. (This notification is triggered by the payload code, using the
  [`AVmPayload_notifyPayloadReady()`](../vm_payload/include/vm_payload.h)
  function.
- `onPayloadFinished()`: The VM payload has exited normally. The exit code of
  the VM (the value returned by [`AVmPayload_main()`](../vm_payload/README.md)
  is supplied as a parameter.
- `onError()`: The VM failed; something went wrong. An error code and
  human-readable message are provided which may help diagnosing the problem.
- `onStopped()`: The VM is no longer running. This is the final notification
  from any VM run, whether or not it was successful. You can run the VM again
  when you want to. A reason code for why the VM stopped is supplied as a
  parameter.

## Communicating with a VM

Once the VM payload has successfully started you will probably want to establish
communication between it and your app.

Only the app that started a VM can connect to it. The VM can accept connections
from the app, but cannot initiate connections to other VMs or the host Android.

The simplest form of communication is using a socket running over the
virtio-vsock protocol.

We suggest that the VM payload should create a listening socket and then trigger the `onPayloadReady()` notification; the app can then connect to the socket.

In the payload this might look like this:

```C++
#include "vm_payload.h"

extern "C" int AVmPayload_main() {
  int fd = socket(AF_VSOCK, SOCK_STREAM, 0);
  // bind, listen
  AVmPayload_notifyPayloadReady();
  // accept, read/write, ...
}
```

And, in the app, like this:

```Java
void onPayloadReady(VirtualMachine vm) {
  ParcelFileDescriptor pfd = vm.connectVsock(port);
  // ...
}
```

