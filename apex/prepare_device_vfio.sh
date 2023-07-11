#!/bin/sh

# Prepare a device for VFIO assignment by binding a VFIO driver to it.

handle_error() {
    echo "Error on line $1"
    exit 1
}
trap 'handle_error $LINENO' ERR

check_user() {
    if [ $(id -u) -ne 0 ];
    then
        echo "run as root"
        exit 1
    fi
}

check_vfio() {
    if [ ! -c "$vfio_dir/vfio" ];
    then
        echo "cannot find $vfio_dir/vfio"
        return 1
    fi

    if [ ! -d "$platform_bus/drivers/vfio-platform" ];
    then
        echo "VFIO-platform is not supported"
        return 1
    fi

    return 0
}

check_device() {
    if [ ! -d "$device_sys" ] || [ -z "$device" ];
    then
        echo "no device $device ($device_sys)"
        return 1
    fi
    return 0
}

get_device_iommu_group() {
    local group=$(basename $(readlink "$device_sys/iommu_group")) || true
    echo "$group"
}

device="$1"
device_sys="/sys/bus/platform/devices/$device"
device_driver="$device_sys/driver"
platform_bus="/sys/bus/platform"
vfio_dir="/dev/vfio"
vfio_noiommu_param="/sys/module/vfio/parameters/enable_unsafe_noiommu_mode"
vfio_reset_required="/sys/module/vfio_platform/parameters/reset_required"

check_user
check_device
check_vfio

group=$(get_device_iommu_group)
if [ -z "$group" ];
then
    echo "$device_sys does not have an IOMMU group"
    echo y > "$vfio_noiommu_param"
fi

# Unbind driver
if [ -e "$device_driver" ] && [ ! $(basename $(readlink "$device_driver")) = "vfio-platform" ];
then
    echo "$device" > "$device_driver/unbind"
fi

# Turn off SELinux to allow virtualizationmanager and crosvm access sysfs
echo "[*WARN*] setenforce=0"
setenforce 0

# Bind to VFIO driver
if [ ! -e "$device_driver" ];
then
    # Samsung IOMMU does not report interrupt remapping support
    echo y > /sys/module/vfio_iommu_type1/parameters/allow_unsafe_interrupts
    # Bind vfio-platform driver
    echo "vfio-platform" > "$device_sys/driver_override"
    echo "$device" > "$platform_bus/drivers_probe"
fi

sleep 2

# Verify new VFIO files
group=$(get_device_iommu_group)
if [ -z "$group" ];
then
    echo "cannot setup VFIO-NOIOMMU for $device_sys"
    exit 1
fi
if [ ! -c "$vfio_dir/$group" ] || [ ! -e "$device_driver" ] || [ ! $(basename $(readlink "$device_driver")) = "vfio-platform" ];
then
    echo "could not bind $device to VFIO platform driver"
    if [ $(cat $vfio_reset_required) = Y ]; then
        echo "VFIO device reset handler must be registered. Either unset $vfio_reset_required, or register a reset handler for $device_sys"
    fi
    exit 1
fi

echo "Device: $device_sys"
echo "IOMMU group: $group"
echo "VFIO group file: $vfio_dir/$group"
echo "Ready!"
