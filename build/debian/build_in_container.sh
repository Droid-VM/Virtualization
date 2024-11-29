#!/bin/bash

if [ -z "$ANDROID_BUILD_TOP" ] ; then
  echo 'ANDROID_BUILD_TOP undefined.'
  echo 'Please `lunch` an Android target, or manually set the variable.'
  exit 1
fi

arch="$(uname -m)"
release_flag=
save_workdir_flag=

while getopts "a:rw" option; do
  case ${option} in
    a)
      arch="$OPTARG"
      ;;
    r)
      release_flag="-r"
      ;;
    w)
      save_workdir_flag="-w"
      ;;
    *)
      echo "Invalid option: $OPTARG" ; exit 1
      ;;
  esac
done

if [[ "$arch" != "aarch64" && "$arch" != "x86_64" ]]; then
  echo "Invalid architecture: $arch" ; exit 1
fi

docker run --privileged -it -v /dev:/dev \
  -v "$ANDROID_BUILD_TOP/packages/modules/Virtualization:/root/Virtualization" \
  --workdir /root/Virtualization/build/debian \
  ubuntu:22.04 \
  bash -c "/root/Virtualization/build/debian/build.sh -a $arch $release_flag $save_workdir_flag || bash"
