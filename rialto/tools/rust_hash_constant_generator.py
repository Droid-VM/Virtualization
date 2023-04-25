#!/usr/bin/env python3
# Copyright (C) 2023 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the 'License');
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#      http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an 'AS IS' BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
"""This script calculates the SHA256 hash of a file and writes it as a constant
in a Rust file.

Usage: python generate_hash_constant.py input_file output_file

Arguments:
- input_file: Path to the input file to be hashed.
- output_file: Path to the output Rust file that will contain the hash constant.
- salt(optional): Salt to be included in the beginning of the data.

The output Rust file will contain a constant named HASH_VALUE, which is a byte
array that contains the SHA256 hash of the input file. The constant can be used
in Rust code by importing the module that contains it and using it in your code.
"""

import hashlib
import argparse

def main():
    parser = argparse.ArgumentParser(
        description='Calculate SHA256 hash of a file and write as a constant' +
        'to a Rust file')
    parser.add_argument('input_file', help='Path to input file')
    parser.add_argument('output_file', help='Path to output Rust file')
    parser.add_argument('--salt', help='Salt to be added to the hash')

    args = parser.parse_args()

    ctx = hashlib.sha256()

    # Add the salt to the beginning of the data
    if args.salt:
        print(args.salt)
        ctx.update(bytes.fromhex(args.salt))

    # Open the input file and read its contents
    with open(args.input_file, 'rb') as f:
        ctx.update(f.read())

    # Calculate the SHA256 hash of the file's contents
    hash_value = ctx.digest()

    # Write the hash value as a Rust constant to the output file
    with open(args.output_file, 'w', encoding="utf-8") as f:
        f.write("#![no_std]\n")
        f.write("#![allow(missing_docs)]\n\n")
        f.write(
            "pub const HASH_VALUE: &[u8] = &[" +
            f"{', '.join(str(x) for x in hash_value)}];\n")


if __name__ == "__main__":
    main()
