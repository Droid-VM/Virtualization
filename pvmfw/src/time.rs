// Copyright 2023, The Android Open Source Project
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

//! Support for tracking time.

#![allow(unused)]

use crate::read_sysreg;
use alloc::vec::Vec;
use spin::mutex::SpinMutex;

/// Initialize clock source.
pub fn init() {}

/// Get current clock tick.
#[inline(always)]
pub fn now() -> usize {
    read_sysreg!("CNTPCT_EL0")
}

/// Measure time spent computing an expression.
#[macro_export]
macro_rules! time_this {
    ($expr:expr) => {{
        let before = $crate::time::now();
        *$crate::time::RECORDED_DEPTH.lock() += 1;
        let r = { $expr };
        *$crate::time::RECORDED_DEPTH.lock() -= 1;
        $crate::time::record_time_since(stringify!($expr), before);
        r
    }};
}

/// log all the recorded times.
pub fn log_recorded_times() {
    let freq = read_sysreg!("CNTFRQ_EL0") as f64 / 1_000_000.; // 1/us
    for (shift, tag, record) in RECORDED_TIMES.lock().iter() {
        let align = " ".repeat(*shift * 2);
        let fname = get_recorded_time_fname(tag);
        let ticks = *record as f64;
        let time = ticks / freq; // us
        log::info!("{time:>7.0} {align}{fname}");
    }
}

static RECORDED_TIMES: SpinMutex<Vec<(usize, &'static str, usize)>> = SpinMutex::new(Vec::new());
/// time_this! recursion tracker.
pub static RECORDED_DEPTH: SpinMutex<usize> = SpinMutex::new(0);

/// Append a time record to be logged in log_recorded_times.
pub fn record_time_delta(expr: &'static str, before: usize, after: usize) {
    RECORDED_TIMES.lock().push((*RECORDED_DEPTH.lock(), expr, after.wrapping_sub(before)))
}

/// Append a time record to be logged in log_recorded_times.
pub fn record_time_since(expr: &'static str, before: usize) {
    record_time_delta(expr, before, now())
}

fn get_recorded_time_fname(tag: &str) -> &str {
    const LOGGERS: [&str; 10] = [
        "error!(",
        "warn!(",
        "info!(",
        "debug!(",
        "trace!(",
        "log::error!(",
        "log::warn!(",
        "log::info!(",
        "log::debug!(",
        "log::trace!(",
    ];
    const PREFIXES: [&str; 3] =
        ["get_hypervisor()", "MEMORY.lock().as_mut().unwrap()", "MEMORY.lock().take().unwrap()"];

    if LOGGERS.iter().any(|&p| tag.starts_with(p)) {
        return tag;
    }

    let search_start = if let Some(prefix) = PREFIXES.iter().find(|&p| tag.starts_with(p)) {
        prefix.len() - 1
    } else {
        0
    };

    let Some(found_end) = tag[search_start..].find('(') else {
        return tag
    };

    &tag[..(search_start + found_end)]
}
