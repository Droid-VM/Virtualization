#!/bin/bash

# First check that we only have one device attached
if [[ $(adb devices | grep -v ^$ | wc -l) -ne 2 ]]; then
  echo "You should have only one device attached"
  echo "Found:"
  echo -e "$(adb devices)"
  exit 1
fi

BENCH_PORT=5666
BYTES_TRANSFER=50331648
HOST_SERIAL=$(adb devices | grep -v ^$ | awk 'FNR==2 {print $1}')

echo "host serial: ${host_serial}"

echo "First of all make sure we run as root"
adb root || exit 1

echo "Now clean up potential mess from the previous run"

TMP_DIR="/data/local/tmp/vsock-debug"

adb shell rm -Rf "${TMP_DIR}" || exit 1

adb shell mkdir "${TMP_DIR}" || exit 1

cat << EOF
Push MicrodroidTestApp.apk and vsock_client to the device.
Note: the vsock_server is packaged inside MicrodroidTestApp.apk

BIG NOTE: this script hard codes cheetah as the product. Change it if you are using different device
EOF

adb push out/target/product/cheetah/system/bin/vsock_client ${TMP_DIR}/vsock_client
adb push out/target/product/cheetah/testcases/MicrodroidTestApp/arm64/MicrodroidTestApp.apk ${TMP_DIR}/MicrodroidTestApp.apk

function start_vm () {
  adb shell /apex/com.android.virt/bin/vm run-app \
    ${TMP_DIR}/MicrodroidTestApp.apk \
    ${2}/MicrodroidTestApp.apk.idsig \
    ${2}/instance.img \
    --instance-id-file ${2}/instance_id \
    --payload-binary-name MicrodroidIdleNativeLib.so \
    --protected \
    --debug full \
    --gki ${1}
}

function get_cid () {
  local selected_cid=$1
  local available_cids=$(adb shell /apex/com.android.virt/bin/vm list | awk 'BEGIN { FS="[:,]" } /cid/ { print $2; }')
  echo "${available_cids}"
}

function connect_vm() {
  local cid=${1}
  local port=${2}
  echo Connecting to CID ${cid} port ${port}
  adb disconnect localhost:${port} 2>/dev/null
  adb forward tcp:${port} vsock:${cid}:5555
  adb connect localhost:${port}
  adb -s localhost:${port} root
  adb -s localhost:${port} wait-for-device
}

function compute_bench () {
  local raw_data_file=${1}
  local data_file=${2}
  min_val=$(adb shell cat "${raw_data_file}" | sort -n | head -1)
  max_val=$(adb shell cat "${raw_data_file}" | sort -n | tail -1)
  size=$(adb shell cat "${raw_data_file}" | wc -l)
  sum=$(adb shell cat "${raw_data_file}" | awk '{sum += $0} END {print sum}')
  avg_val=$(bc -l <<< $sum/$size)
  sq_sum=0
  for val in $(adb shell cat "${raw_data_file}"); do
    tmp_val=$(bc -l <<<"($val-$avg_val)*($val-$avg_val)")
    sq_sum=$(bc -l <<<"$sq_sum+$tmp_val")
  done
  st_dev=$(bc -l <<<"sqrt($sq_sum / ($size - 1))")
  median=$(adb shell cat "${raw_data_file}" | sort -n | awk '{ a[NR]=$1; } END { if (NR % 2 == 1) print a[(NR + 1) / 2]; else print (a[NR / 2] + a[NR / 2 + 1]) / 2;}')
  adb shell "echo \"min ${min_val}\" >> ${data_file}"
  adb shell "echo \"max ${max_val}\" >> ${data_file}"
  adb shell "echo \"avg ${avg_val}\" >> ${data_file}"
  adb shell "echo \"st_dev ${st_dev}\" >> ${data_file}"
  adb shell "echo \"median ${median}\" >> ${data_file}"
}

function run_vsock_bench () {
  ver="${1}"
  echo "Running vsock benchmark for ${ver}"

  local work_dir="${TMP_DIR}/${ver}"

  echo "Creating ${work_dir}"
  adb shell mkdir "${work_dir}"

  start_vm "${ver}" "${work_dir}" >/dev/null 2>/dev/null &

  echo "Sleep to make sure VM is running"
  sleep 5

  local vm_cid=$(get_cid)
  echo "VM CID : ${vm_cid}"
  local vm_port=9876
  connect_vm ${vm_cid} ${vm_port}

  for iter in {1..5}; do
    echo ${iter}
    local cur_port=$((${BENCH_PORT} + ${iter}))
    adb -s localhost:${vm_port} shell /mnt/apk/bin/vsock_server ${cur_port} ${BYTES_TRANSFER} &
    sleep 1
    adb -s ${HOST_SERIAL} shell "${TMP_DIR}/vsock_client ${vm_cid} ${cur_port} ${BYTES_TRANSFER} >> ${work_dir}/data.raw"
  done;

  adb disconnect localhost:${vm_port}
  echo "Killing virtmgr which should corresponding VM"
  adb shell 'killall virtmgr'

  compute_bench ${work_dir}/data.raw ${work_dir}/data
}

run_vsock_bench "microdroid_14_61"
run_vsock_bench "microdroid"

echo "Just in case disconnect adb session to a VM"
adb disconnect localhost:9876

echo "Just in case again killing virtmgr processes which should corresponding VMs"
adb shell 'killall virtmgr'

echo "microdroid_14_61 stats:"
adb shell cat "${TMP_DIR}/microdroid_14_61/data"

echo "microdroid stats:"
adb shell cat "${TMP_DIR}/microdroid/data"
