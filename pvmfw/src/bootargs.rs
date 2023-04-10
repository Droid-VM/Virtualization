// Copyright 2022, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! routins for parsing bootargs

use core::ffi::CStr;

/// A single boot argument ex: "panic", "init=", or "foo=1,2,3".
#[derive(Debug)]
pub struct BootArg<'a> {
    /// Name of the arg
    pub name: &'a [u8],
    /// Value of the arg if any. This includes the '=' character.
    pub value: Option<&'a [u8]>,
}

/// Iterator that iteratos over bootargs
pub struct BootArgsIterator<'a> {
    bootargs: &'a [u8],
    index: usize,
}

impl<'a> BootArgsIterator<'a> {
    /// Creates a new iterator from the raw boot args
    pub fn new(bootargs: &'a CStr) -> Self {
        Self { bootargs: bootargs.to_bytes(), index: 0 }
    }

    // skips spaces to find the next name
    fn skip_spaces(&mut self) -> usize {
        let i = &mut self.index;
        while let Some(c) = self.bootargs.get(*i) {
            if *c == b' ' {
                *i += 1;
            } else {
                break;
            }
        }
        *i
    }

    // skips until the name ends with a space or =
    fn find_name_end(&mut self) -> usize {
        let i = &mut self.index;
        while let Some(c) = self.bootargs.get(*i) {
            if *c == b' ' || *c == b'=' {
                break;
            } else {
                *i += 1;
            }
        }
        *i
    }

    // skips until the end of value is reached. a value can have spaces if quoted. quote character
    // can't be escaped.
    fn find_value_end(&mut self) -> usize {
        let index = &mut self.index;
        let mut in_quote = false;
        while let Some(c) = self.bootargs.get(*index) {
            if *c == b'\"' {
                in_quote = !in_quote;
            }
            if *c == b' ' && !in_quote {
                break;
            }
            *index += 1;
        }
        *index
    }
}

impl<'a> Iterator for BootArgsIterator<'a> {
    type Item = BootArg<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.skip_spaces();
        self.bootargs.get(self.index)?; // early return if out of bounds
        let name = &self.bootargs[self.index..self.find_name_end()];
        // there's value if the next character is =
        let value = match self.bootargs.get(self.index) {
            Some(c) if *c == b'=' => Some(&self.bootargs[self.index..self.find_value_end()]),
            _ => None,
        };
        Some(BootArg { name, value })
    }
}
