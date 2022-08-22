"""This module run avbtool with some options to generate vbmeta bootconfig."""
#!/usr/bin/env python3

import sys
import subprocess

def main(args):
    """Run avbotool generate vbmeta related bootconfigs"""
    avbtool = args[0]
    vbmeta_img = args[1]
    # Keep in sync with avbtool
    avb_version = -1
    default_hash_algorithm = 'sha256'
    size = 0
    vbmeta_hash = '#'

    with subprocess.Popen([avbtool, 'version'],
                            stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT) as proc:
        stdout, _stderr = proc.communicate()
        avb_version = stdout.decode("utf-8").split(" ")[1].strip()

    with subprocess.Popen([avbtool, 'info_image', '--image', vbmeta_img],
                           stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT) as proc:
        stdout, _stderr = proc.communicate()
        for line in stdout.decode("utf-8").split("\n"):
            line = line.split(":")
            if line[0] in ['Header Block',
                'Authentication Block', 'Auxiliary Block']:
                size += int(line[1].strip()[0:-6])

    with subprocess.Popen([avbtool, 'calculate_vbmeta_digest',
                            '--image', vbmeta_img,
                            '--hash_algorithm', default_hash_algorithm],
                           stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT) as proc:
        stdout, _stderr = proc.communicate()
        vbmeta_hash = stdout.decode("utf-8").strip()

    print(f'androidboot.vbmeta.size = {size}')
    print(f'androidboot.vbmeta.digest = \"{vbmeta_hash}\"')
    print(f'androidboot.vbmeta.hash_alg = \"{default_hash_algorithm}\"')
    print(f'androidboot.vbmeta.avb_version = \"{avb_version}\"')

## Main body
if __name__ == '__main__':
    main(sys.argv[1:])
