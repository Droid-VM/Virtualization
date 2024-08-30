#!/bin/bash

pushd $(dirname $0) > /dev/null

echo Get Debian image and dependencies...
wget https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-nocloud-arm64.raw -O debian.img
wget https://github.com/tsl0922/ttyd/releases/download/1.7.7/ttyd.aarch64 -O ttyd

echo Customize the image...
virt-customize --commands-from-file commands -a debian.img

asset_dir=../assets/linux
mkdir -p ${asset_dir}

echo Copy files...
tar czvS -f ${asset_dir}/images.tar.gz *.img
cp vm_config.json ${asset_dir}

echo Calculating hash...
hash=$(cat ${asset_dir}/images.tar.gz ${asset_dir}/vm_config.json | sha1sum | cut -d' ' -f 1)
echo ${hash} > ${asset_dir}/version

popd > /dev/null