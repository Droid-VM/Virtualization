# This script runs 256 MB file benchmark, both on host and on authfs.
# Usage:
# $ source build/envsetup.sh
# $ lunch <target>-userdebug
# $ . packages/modules/Virtualization/tests/benchmark/benchmark_example.sh

adb root && \
m fs_benchmark MicrodroidFilesystemBenchmarkApp fsverity && \
adb shell 'rm -rf /data/local/tmp/virt' && \
adb shell 'mkdir -p /data/local/tmp/virt' && \
adb push $OUT/system/bin/fs_benchmark /data/local/tmp && \
adb install $OUT/system/app/MicrodroidFilesystemBenchmarkApp/MicrodroidFilesystemBenchmarkApp.apk && \
dd if=/dev/zero of=/tmp/testcase bs=1048576 count=256 && \
fsverity sign /tmp/testcase /tmp/testcase.fsv_sig --key=packages/modules/Virtualization/tests/benchmark/assets/benchmark.pem \
    --out-merkle-tree=/tmp/testcase.merkle_dump --cert=packages/modules/Virtualization/tests/benchmark/assets/benchmark.x509.pem && \
adb shell 'dd if=/dev/zero of=/data/local/tmp/testcase bs=1048576 count=256' && \
adb push /tmp/testcase.fsv_sig /tmp/testcase.merkle_dump /data/local/tmp && \
(adb shell 'exec 3</data/local/tmp/testcase 4</data/local/tmp/testcase.merkle_dump 5</data/local/tmp/testcase.fsv_sig 6</data/local/tmp/testcase /apex/com.android.virt/bin/fd_server --ro-fds 3:4:5 --ro-fds 6' & ) && \
result=$(adb shell "/apex/com.android.virt/bin/vm run-app --debug full --daemonize --log /data/local/tmp/virt/log.txt $(adb shell pm path com.android.microdroid.benchmark | cut -d':' -f2) /data/local/tmp/virt/MicrodroidFilesystemBenchmarkApp.apk.idsig /data/local/tmp/virt/instance.img assets/vm_config.json") && \
cid=$(echo $result | grep -P "with CID \d+" --only-matching --color=none | cut -d' ' -f3) && \
echo "CID IS $cid" && \
echo "RUNNING HOST TEST" && \
adb shell 'dd if=/dev/zero of=/data/local/tmp/testcase_host bs=1048576 count=256' && \
adb shell '/data/local/tmp/fs_benchmark /data/local/tmp/testcase_host 268435456 both 5' && \
echo "RUNNING GUEST TEST" && \
adb forward tcp:8000 vsock:$cid:5555 && \
adb connect localhost:8000 && \
adb -s localhost:8000 root && \
sleep 10 && \
adb -s localhost:8000 shell "mkdir -p /data/local/tmp/authfs" && \
(adb -s localhost:8000 shell "/system/bin/authfs /data/local/tmp/authfs --cid 2 --remote-ro-file 10:3:/mnt/apk/assets/benchmark.x509.der --remote-ro-file-unverified 11:6" &) && \
adb -s localhost:8000 push $OUT/system/bin/fs_benchmark /data/local/tmp && \
adb -s localhost:8000 shell "/data/local/tmp/fs_benchmark /data/local/tmp/authfs/10 268435456 read 5" &&
adb -s localhost:8000 shell "/data/local/tmp/fs_benchmark /data/local/tmp/authfs/11 268435456 read 5"
