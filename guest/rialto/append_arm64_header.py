# Copyright (C) 2024 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#      http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
"""Append ARM64 header to the given file.

Usage: append_arm64_header.py <original_kernel_file> <text_offset> <new_file>
"""

import struct
import sys

"""Append ARM64 header to the given file.

Usage: append_arm64_header.py <original_kernel_file> <text_offset> <new_file>
"""

def append_arm64_header(original_kernel_file, text_offset, new_filename):
    """
    Appends an Arm64ImageHeader to the beginning of the given binary file
    and saves the result to a new file

    Args:
        original_kernel_file (str): The path to the original kernel file.
        text_offset (int): The offset to the text section in the image.
        new_filename (str): The path to the new file where the modified
          image will be saved
    """

    with open(original_kernel_file, "rb") as f:
        original_data = f.read()

    header_data = {
        "code0": 0,
        "code1": 0,
        "text_offset": 0,
        "image_size": len(original_data),
        "flags": 0,
        "res2": 0,
        "res3": 0,
        "res4": 0,
        "magic": 0x644d5241,
        "res5": 0,
    }

    print(header_data)

    # Pack the header data into a byte string (little-endian)
    header_format = "<IIQQQQQQII"  # Adjust format string if necessary
    header_bytes = struct.pack(header_format,
                               header_data["code0"],
                               header_data["code1"],
                               header_data["text_offset"],
                               header_data["image_size"],
                               header_data["flags"],
                               header_data["res2"],
                               header_data["res3"],
                               header_data["res4"],
                               header_data["magic"],
                               header_data["res5"])

    assert len(header_bytes) == 64, \
        f"Header size is {len(header_bytes)}, expected 64 bytes"
    
    print(header_bytes.hex())
    # Write the header and original data to the new file
    with open(new_filename, "wb") as f:
        f.write(header_bytes)
        f.write(original_data)

if __name__ == "__main__":
    if len(sys.argv) != 4:
        print(__doc__)  # Print the docstring for usage instructions
        sys.exit(1)

    original_kernel_file = sys.argv[1]
    text_offset = int(sys.argv[2], 16)  # Convert hex string to integer
    print(text_offset)
    new_filename = sys.argv[3]
    append_arm64_header(original_kernel_file, text_offset, new_filename)
