#!/system/bin/sh

set -e

function copy_files() {
  cp /sdcard/vm_config.json /data/local/tmp
  cp /sdcard/vmlinuz /data/local/tmp
  cp /sdcard/chromiumos_test_image.bin /data/local/tmp
  chmod 666 /data/local/tmp/vm_config.json
  chmod 666 /data/local/tmp/chromiumos_test_image.bin
  chmod 666 /data/local/tmp/vmlinuz
}
copy_files
setprop debug.custom_vm_setup.start false