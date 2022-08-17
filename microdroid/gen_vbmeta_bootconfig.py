
#!/usr/bin/env python3
"""
foobar
"""
import sys
import subprocess
def eprint(*args, **kwargs):
    print(*args, file=sys.stderr, **kwargs)
def main(args):
    avbtool = args[0]
    vbmeta_img = args[1]
    out = subprocess.Popen([avbtool, 'info_image', '--image', vbmeta_img],
                           stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT)
    stdout, stderr = out.communicate()
    size = 0
    for line in stdout.decode("utf-8").split("\n"):
        line = line.split(":")
        if line[0] in ['Header Block', 'Authentication Block', 'Auxiliary Block']:
            size += int(line[1].strip()[0:-6])
    out = subprocess.Popen([avbtool, 'calculate_vbmeta_digest', '--image', vbmeta_img],
                           stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT)
    stdout, stderr = out.communicate()
    hash = stdout.decode("utf-8").strip()
    print("androidboot.vbmeta.size = {}".format(size))
    print("androidboot.vbmeta.digest = \"{}\"".format(hash))
# Main body
if __name__ == '__main__':
    main(sys.argv[1:])
