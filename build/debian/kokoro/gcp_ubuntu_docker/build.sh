<<<<<<< PATCH SET (1e439e Add x86_64 image to kokoro build)
||||||| BASE
#!/bin/bash

set -e

cd "${KOKORO_ARTIFACTS_DIR}/git/avf/build/debian/"
sudo losetup -D
grep vmx /proc/cpuinfo || true
sudo ./build.sh
tar czvS -f ${KOKORO_ARTIFACTS_DIR}/images.tar.gz image.raw

mkdir -p ${KOKORO_ARTIFACTS_DIR}/logs
# TODO(b/372162211): Find exact location of log without breaking kokoro build.
cp -r /var/log/fai/*/last/* ${KOKORO_ARTIFACTS_DIR}/logs || true
=======
#!/bin/bash

set -e

cd "${KOKORO_ARTIFACTS_DIR}/git/avf/build/debian/"
sudo losetup -D
grep vmx /proc/cpuinfo || true
sudo ./build.sh
tar czvS -f ${KOKORO_ARTIFACTS_DIR}/images.tar.gz image.raw

mkdir -p ${KOKORO_ARTIFACTS_DIR}/logs
sudo cp -r /var/log/fai/* ${KOKORO_ARTIFACTS_DIR}/logs || true
>>>>>>> BASE      (feb78a Merge "Remove dependency on once_cell where it isn't actuall)
