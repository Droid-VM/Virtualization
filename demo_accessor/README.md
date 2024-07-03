# IAccessor implementation demo app

IAccessor allows AIDL service in a VM can be accessed via servicemanager.
To do so, VM owners should also provide IAccessor implementation.

This demo apex provides the minimum setup for IAccessor as follows:
  - accessor_demo: Sample implementation of IAccessor, which is expected to
    launch VM and returns the Vsock connection of service in the VM.
  - AccessorVmApp: Sample app that conatins VM payload.
