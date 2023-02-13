avf_debug_policy_root="/sys/firmware/devicetree/base/avf"

# If VM is debuggable or debug policy says so, send logs to outside ot the VM via the serial console.
# Otherwise logs are internally consumed at /dev/null
if [[ "$(getprop ro.boot.microdroid.debuggable)" == "1"
        || $(xxd -p "${avf_debug_policy_root}/guest/common/log") == "00000001" ]]; then
    setprop ro.log.file_logger.path /dev/hvc2
else
    setprop ro.log.file_logger.path /dev/null
fi

if [[ "$(getprop ro.boot.adb.enabled)" == "1"
        || $(xxd -p "${avf_debug_policy_root}/guest/microdroid/adb") == "00000001" ]]; then
    setprop microdroid_manager.adbd.enabled 1
fi

if [[ "$(getprop ro.boot.microdroid.debuggable)" == "1"
        || $(xxd -p "${avf_debug_policy_root}/guest/microdroid/adb_root") == "00000001" ]]; then
    setprop ro.debuggable 1
else
    setprop ro.debuggable 0
fi
