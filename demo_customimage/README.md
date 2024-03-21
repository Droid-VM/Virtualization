# Custom image demo app

## Building & Installing

Add `CustomImageDemoApp` into `PRODUCT_PACKAGES` and then `m`

You can also explicitly grant or revoke the permission, e.g.
```
adb shell pm grant com.android.virtualization.custom_image_demo android.permission.USE_CUSTOM_VIRTUAL_MACHINE
adb shell pm grant com.android.virtualization.custom_image_demo android.permission.MANAGE_VIRTUAL_MACHINE
```

## Running

Copy vm config json file to /data/local/tmp/vm_config.json.
And then, run the app, check log meesage.
