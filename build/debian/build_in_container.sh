#!/bin/bash

full_path=$(dirname $(realpath $0))
docker run --privileged -it -v ${full_path}:/root/debian -v /dev:/dev ubuntu:22.04 /root/debian/build.sh
