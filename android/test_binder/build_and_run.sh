#!/bin/bash

# Check if it's sourced
(
  [[ -n $ZSH_VERSION && $ZSH_EVAL_CONTEXT =~ :file$ ]] ||
  [[ -n $KSH_VERSION && "$(cd -- "$(dirname -- "$0")" && pwd -P)/$(basename -- "$0")" != "$(cd -- "$(dirname -- "${.sh.file}")" && pwd -P)/$(basename -- "${.sh.file}")" ]] ||
  [[ -n $BASH_VERSION ]] && (return 0 2>/dev/null)
) && sourced=1 || sourced=0

if [[ "${sourced}" == "0" ]]; then
  echo "Source this script" >&2
  exit 1
fi

if [[ -z "${OUT}" ]]; then
  echo "\$OUT is missing. Have you sourced {env,rbe}setup.sh?" >&2
  return
fi

function main() {
  m javaclient javabackend rustbackend || return 0

  adb root && adb remount -R && adb wait-for-device

  adb push $OUT/system/app/javabackend/* /system/app/javabackend/
  adb push $OUT/system/priv-app/javaclient/* /system/priv-app/javaclient/
  # copy symlink
  adb push $OUT/system/lib64/libtest_binder_jni.so /system/lib64/
  adb push $OUT/system/bin/rustbackend /data/local/tmp/rustbackend

  adb reboot && adb wait-for-device && adb root

  adb shell setenforce 0
}

set -x
main
set +x
