# IAccessor implementation demo app

IAccessor allows AIDL service in a VM can be accessed via servicemanager.
To do so, VM owners should also provide IAccessor implementation.

This demo apex provides the minimum setup for IAccessor as follows:
  - accessor_demo: Sample implementation of IAccessor, which is expected to
    launch VM and returns the Vsock connection of service in the VM.
  - AccessorVmApp: Sample app that conatins VM payload.

## Build

You need to do envsetup.sh
```shell
m com.android.virt.accessor_demo
```

## Install (requires userdebug build)

For very first install,

```shell
adb root
adb remount -R        # To push apex to /system_ext. May reboot
adb wait-for-device
adb push $ANDROID_PRODUCT_OUT/system_ext/com.android.virt.accessor_demo.apex /system_ext/apex
adb reboot            # Ensure that newly pushed apex at /system_ext is installed
adb wait-for-device
```

After that, you can simply use `adb install`

```shell
adb install $ANDROID_PRODUCT_OUT/system_ext/com.android.virt.accessor_demo.apex
adb reboot
```