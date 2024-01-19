// Copyright 2024 The Android Open Source Project
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

//! CLI for converting file system and FDT, and vice versa.

use clap::Parser;
use log::{error, warn};
use std::path::PathBuf;

/// Option parser
#[derive(Parser, Debug)]
struct Opt {
    /// Dir path to parse from
    dir_path: PathBuf,

    /// FDT file path for writing
    fdt_file_path: PathBuf,

    /// FDT max size
    #[arg(default_value = "1024")]
    fdt_max_size: usize,
}

fn main() {
    env_logger::init();

    let opt = Opt::parse();
    let res = fsfdt::fs_to_fdt(&opt.dir_path, &opt.fdt_file_path, opt.fdt_max_size);
    if Err(e) = res {
        error!("{e:?}");
        std::process::exit(1);
    }
}
