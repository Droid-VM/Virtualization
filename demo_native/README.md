# Microdroid Demo app in C++

This app is a demonstration of how to create a VM and run payload in it, in C++.

## Building

```sh
source build/envsetup.sh
choosecombo 1 aosp_arm64 userdebug
m MicrodroidTestApp
m vm_demo_native
```

`MicrodroidTestApp` is the application what will be running in the VM. Actually, we will run a
native shared library `MicrodroidTestNativeLib.so` from the APK.

`vm_demo_native` runs on the host (i.e. Android). Its job is to start the VM and connect to the
native shared lib and do some work using the lib. Specifically, we will call an AIDL method
`addInteger` which adds two integers and returns the sum. The computation will be done in the VM.

## Installing

```sh
adb push out/target/product/generic_arm64/testcases/MicrodroidTestApp/arm64/MicrodroidTestApp.apk \
  /data/local/tmp/
adb push out/target/product/generic_arm64/system/bin/vm_demo_native /data/local/tmp/
```

## Running

```sh
adb root
adb shell setenforce 0
adb shell /data/local/tmp/vm_demo_native
```

Rooting and selinux disabling are required just because there's no sepolicy configured for this demo
application. For production, you need to set the sepolicy up correctly.

## Expected output

```sh
[2023-05-09T22:58:19.956073260+09:00 INFO  crosvm] crosvm started.
[2023-05-09T22:58:19.959372820+09:00 INFO  crosvm] CLI arguments parsed.
...
The answer from VM is 30
Done
```
