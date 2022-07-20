#!/usr/bin/env python3
"""
foobar
"""

import sys
import os

def eprint(*args, **kwargs):
    print(*args, file=sys.stderr, **kwargs)

def output(bytes):
    sys.stdout.buffer.write(bytes)

def output_file(path):
    with open(path, "rb") as f:
        output(f.read())

def gen_checksum(path):
    checksum = 0
    with open(path, "rb") as f:
        for val in f.read():
            checksum = (checksum + val) & 0xFFFFFFFF
    return checksum

def main(args):
    ramdisk = args[0]
    vendor_ramdisk = args[1]
    bootconfig = args[2]

    ramdisk_size = os.path.getsize(ramdisk)
    vendor_ramdisk_size = os.path.getsize(vendor_ramdisk)
    bootconfig_size = os.path.getsize(bootconfig)
    padding_size = (4 - ((ramdisk_size + vendor_ramdisk_size + bootconfig_size) % 4)) % 4

    checksum = gen_checksum(bootconfig)

    eprint("ramdisk_size = {}".format(ramdisk_size))
    eprint("vendor_ramdisk_size = {}".format(vendor_ramdisk_size))
    eprint("bootconfig_size = {}".format(bootconfig_size))
    eprint("padding_size = {}".format(padding_size))
    eprint("checksum = {0:08x}".format(checksum))

    # Format:
    # [initrd][bootconfig][4-byte padding][size(le32)][checksum(le32)][#BOOTCONFIGn]

    output_file(ramdisk)
    output_file(vendor_ramdisk)
    output_file(bootconfig)
    output(bytes(padding_size))
    output((bootconfig_size + padding_size).to_bytes(4, byteorder='little'))
    output(checksum.to_bytes(4, byteorder='little'))
    output(bytes("#BOOTCONFIG\n", encoding='utf-8'))

# Main body
if __name__ == '__main__':
    main(sys.argv[1:])
