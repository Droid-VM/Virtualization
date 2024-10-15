#!/bin/bash

set -e

cd "${KOKORO_ARTIFACTS_DIR}/git/avf/build/debian/"
sudo losetup -D
grep vmx /proc/cpuinfo || true
sudo ./build.sh
sudo ./build.sh -a x86_64 image-x86_64.raw
tar czvS -f ${KOKORO_ARTIFACTS_DIR}/images.tar.gz image.raw image-x86_64.raw

mkdir -p ${KOKORO_ARTIFACTS_DIR}/logs
# TODO(b/372162211): Find exact location of log without breaking kokoro build.
cp -r /var/log/fai/*/last/* ${KOKORO_ARTIFACTS_DIR}/logs || true
