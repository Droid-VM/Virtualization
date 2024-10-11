#!/bin/bash

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <images.tar.gz path>"
  exit 1
fi

pushd $(dirname $0) > /dev/null
asset_dir=./assets/linux
mkdir -p ${asset_dir}

cp $1 ${asset_dir}/images.tar.gz
cp vm_config.json ${asset_dir}

echo Calculating hash...
hash=$(cat ${asset_dir}/images.tar.gz ${asset_dir}/vm_config.json | sha1sum | cut -d' ' -f 1)
echo ${hash} > ${asset_dir}/hash

popd > /dev/null
echo Cleaning up...
rm -rf ${tempdir}
