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

use anyhow::Result;
use std::fs;

pub struct CpuTime {
    pub user: i64,
    pub nice: i64,
    pub sys: i64,
    pub idle: i64,
}

pub struct MemInfo {
    pub total: i64,
    pub free: i64,
    pub available: i64,
    pub buffer: i64,
    pub cached: i64,
}

pub fn get_cpu_time() -> Result<CpuTime> {
    let proc_stat = fs::read_to_string("/proc/stat")?;
    let data_list: Vec<_> = proc_stat.split(' ').collect();
    let cpu_time = CpuTime {
        user: data_list[2].parse()?,
        nice: data_list[3].parse()?,
        sys: data_list[4].parse()?,
        idle: data_list[5].parse()?,
    };
    Ok(cpu_time)
}

pub fn get_mem_info() -> Result<MemInfo> {
    let proc_mem_info = fs::read_to_string("/proc/meminfo")?;
    let data_list: Vec<_> = proc_mem_info
        .trim()
        .split('\n')
        .map(|s| s.split(':').last().unwrap().trim())
        .map(|s| &s[..s.len() - 3])
        .collect();

    let mem_info = MemInfo {
        total: data_list[0].parse()?,
        free: data_list[1].parse()?,
        available: data_list[2].parse()?,
        buffer: data_list[3].parse()?,
        cached: data_list[4].parse()?,
    };
    Ok(mem_info)
}
