# Enabling early console for protected VMs

In (very rare ;P) occasions ones patch can make a VM crash or hang before the
console is available. This page explains how one can enable early console to
debug such failures.

## arm64

In case of arm64 you need to pass the following option to the kernel cmdline
`earlycon=uart8250,mmio,0x3f8`. Additionally, it might be useful to also pass
`keep_bootcon` option which will tell kernel not to unregister the boot console.

One way to do this would be to apply the following patch to
`virtualizationmanager/src/aidl.rs`:

```
TODO(ioffe): put the patch here
```

Once you applied the patch, you will need to rebuild the com.android.virt APEX
and push it to the device:

```
$ banchan com.android.virt aosp_arm64
$ UNBUNDLED_BUILD_SDKS_FROM_SOURCE=true m apps_only dist
$ adb install --force-non-staged out/dist/com.android.virt.apex
```

If you are debugging a protected VM, then you also need to take care of pvmfw,
which unmaps the UART console. There are two different ways you can achieve
this:

1. Skip running pvmfw by passing `--protected-vm-without-firmware` to crosvm.

Here is a sample diff that you can apply to
`virtualizationmanager/src/crosvm.rs`:

```
TODO(ioffe): and another patch
```

After that build and push the com.android.virt APEX on the device and set the
`hypervisor.pvmfw.path` sysprop to `"none"`:

```
$ banchan com.android.virt aosp_arm64
$ UNBUNDLED_BUILD_SDKS_FROM_SOURCE=true m apps_only dist
$ adb install --force-non-staged out/dist/com.android.virt.apex
$ adb root
$ adb shell setprop hypervisor.pvmfw.path none
```

2. Another option is to comment out the unmapping logic in pvmfw.

Here is a sample diff that you can apply to `pvmfw/src/entry.rs`:

```
TODO(ioffe): and another one
```

After that, follow these instructions to build & push the pvmfw binary on the
device.

## x86_64

In case of x86_64 you need to pass the following option to the kernel cmdline
`earlycon=uart8250,io,0x3f8`. Again, it might be useful to also pass the
`keep_bootcon` option as well

Here is a sample diff you can apply to `virtualizationmanager/src/aidl.rs`:

```
TODO(ioffe): put the patch here
```

Once you applied the patch, you will need to rebuild the com.android.virt APEX
and push it to the device:

```
$ banchan com.android.virt aosp_x86_64
$ UNBUNDLED_BUILD_SDKS_FROM_SOURCE=true m apps_only dist
$ adb install --force-non-staged out/dist/com.android.virt.apex
```

