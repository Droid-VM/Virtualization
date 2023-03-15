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

//! Safe MMIO_GUARD support.

use core::{fmt, result};
use psci::smccc;

const SIZE_4KB: usize = 4 << 10;
const VENDOR_HYP_KVM_MMIO_GUARD_INFO_FUNC_ID: u32 = 0xc6000005;
const VENDOR_HYP_KVM_MMIO_GUARD_ENROLL_FUNC_ID: u32 = 0xc6000006;
const VENDOR_HYP_KVM_MMIO_GUARD_MAP_FUNC_ID: u32 = 0xc6000007;

#[derive(Debug, Clone)]
pub enum Error {
    /// Failed the necessary MMIO_GUARD_ENROLL call.
    EnrollFailed,
    /// Failed to obtain the MMIO_GUARD granule size.
    InfoFailed,
    /// Failed to MMIO_GUARD_MAP a page.
    MapFailed,
    /// The MMIO_GUARD granule used by the hypervisor is not supported.
    UnsupportedGranule(u64),
}

type Result<T> = result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::EnrollFailed => write!(f, "Failed to enroll into MMIO_GUARD"),
            Self::InfoFailed => write!(f, "Failed to get the MMIO_GUARD granule"),
            Self::MapFailed => write!(f, "Failed to MMIO_GUARD map"),
            Self::UnsupportedGranule(g) => write!(f, "Unsupported MMIO_GUARD granule: {g}"),
        }
    }
}

pub fn init() -> Result<()> {
    mmio_guard_enroll()?;
    let mmio_granule = mmio_guard_info()?;
    if mmio_granule != SIZE_4KB as u64 {
        return Err(Error::UnsupportedGranule(mmio_granule));
    }
    Ok(())
}

fn mmio_guard_enroll() -> Result<()> {
    let args = [0u64; 17];

    checked_hvc64(VENDOR_HYP_KVM_MMIO_GUARD_ENROLL_FUNC_ID, args).ok_or(Error::EnrollFailed)?;
    Ok(())
}

fn mmio_guard_info() -> Result<u64> {
    let args = [0u64; 17];

    checked_hvc64(VENDOR_HYP_KVM_MMIO_GUARD_INFO_FUNC_ID, args).ok_or(Error::InfoFailed)
}

pub fn map(addr: usize) -> Result<()> {
    mmio_guard_map(addr)
}

fn mmio_guard_map(addr: usize) -> Result<()> {
    let mut args = [0u64; 17];
    args[0] = (addr & !(SIZE_4KB - 1)) as u64;

    checked_hvc64(VENDOR_HYP_KVM_MMIO_GUARD_MAP_FUNC_ID, args).ok_or(Error::MapFailed)?;
    Ok(())
}

fn checked_hvc64(function_id: u32, args: [u64; 17]) -> Option<u64> {
    let ret = smccc::hvc64(function_id, args)[0];
    if (ret as i64) < 0 {
        None
    } else {
        Some(ret)
    }
}
