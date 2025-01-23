#PVMFW_BIN=${ANDROID_PRODUCT_OUT}/system/etc/pvmfw.bin
#DICE=${ANDROID_BUILD_TOP}/packages/modules/Virtualization/tests/pvmfw/assets/bcc.dat
#DICE=${ANDROID_BUILD_TOP}/packages/modules/Virtualization/guest/pvmfw/pvmfw_config_1_3.bin

#pvmfw-tool custom_pvmfw ${PVMFW_BIN} ${DICE}

cp ${ANDROID_PRODUCT_OUT}/system/etc/pvmfw.bin custom_pvmfw
truncate --size="%4096" custom_pvmfw
cat custom_pvmfw pvmfw_config_1_3.bin > custom_pvmfw_1_3
adb push custom_pvmfw_1_3 /data/local/tmp/pvmfw
#adb push custom_pvmfw /data/local/tmp/pvmfw

#adb push custom_pvmfw /data/local/tmp/pvmfw
#adb push ${ANDROID_PRODUCT_OUT}/system/etc/pvmfw.bin /data/local/tmp/pvmfw

adb shell /apex/com.android.virt/bin/vm run --cpu-topology match_host -p --debug full --enable-earlycon /data/local/tmp/config_trusty_vm.json
#adb shell /system_ext/bin/trusty_security_vm_launcher \
        #--kernel /system_ext/etc/hw/desktop.lk.bin \
        #--load-kernel-as-bootloader \
        #--memory-size-mib 192 \
        #--cpu-topology match-host \
        #--protected

#adb push ${ANDROID_PRODUCT_OUT}/system/etc/pvmfw.bin /data/local/tmp/pvmfw
