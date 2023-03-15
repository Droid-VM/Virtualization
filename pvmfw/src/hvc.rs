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

//! Wrappers around calls to the hypervisor.

pub mod trng;

use mmio_guard::smccc::{self, checked_hvc64, checked_hvc64_expect_zero};

const ARM_SMCCC_TRNG_VERSION: u32 = 0x8400_0050;
#[allow(dead_code)]
const ARM_SMCCC_TRNG_FEATURES: u32 = 0x8400_0051;
#[allow(dead_code)]
const ARM_SMCCC_TRNG_GET_UUID: u32 = 0x8400_0052;
#[allow(dead_code)]
const ARM_SMCCC_TRNG_RND32: u32 = 0x8400_0053;
const ARM_SMCCC_TRNG_RND64: u32 = 0xc400_0053;
const ARM_SMCCC_KVM_FUNC_HYP_MEMINFO: u32 = 0xc6000002;
const ARM_SMCCC_KVM_FUNC_MEM_SHARE: u32 = 0xc6000003;
const ARM_SMCCC_KVM_FUNC_MEM_UNSHARE: u32 = 0xc6000004;

/// Queries the memory protection parameters for a protected virtual machine.
///
/// Returns the memory protection granule size in bytes.
pub fn kvm_hyp_meminfo() -> smccc::Result<u64> {
    let args = [0u64; 17];
    checked_hvc64(ARM_SMCCC_KVM_FUNC_HYP_MEMINFO, args)
}

/// Shares a region of memory with the KVM host, granting it read, write and execute permissions.
/// The size of the region is equal to the memory protection granule returned by [`hyp_meminfo`].
pub fn kvm_mem_share(base_ipa: u64) -> smccc::Result<()> {
    let mut args = [0u64; 17];
    args[0] = base_ipa;

    checked_hvc64_expect_zero(ARM_SMCCC_KVM_FUNC_MEM_SHARE, args)
}

/// Revokes access permission from the KVM host to a memory region previously shared with
/// [`mem_share`]. The size of the region is equal to the memory protection granule returned by
/// [`hyp_meminfo`].
pub fn kvm_mem_unshare(base_ipa: u64) -> smccc::Result<()> {
    let mut args = [0u64; 17];
    args[0] = base_ipa;

    checked_hvc64_expect_zero(ARM_SMCCC_KVM_FUNC_MEM_UNSHARE, args)
}

/// Returns the (major, minor) version tuple, as defined by the SMCCC TRNG.
pub fn trng_version() -> trng::Result<(u16, u16)> {
    let args = [0u64; 17];

    let version = trng::hvc64(ARM_SMCCC_TRNG_VERSION, args)?[0];
    Ok(((version >> 16) as u16, version as u16))
}

pub type TrngRng64Entropy = (u64, u64, u64);

pub fn trng_rnd64(nbits: u64) -> trng::Result<TrngRng64Entropy> {
    let mut args = [0u64; 17];
    args[0] = nbits;

    let regs = trng::hvc64_expect_zero(ARM_SMCCC_TRNG_RND64, args)?;

    Ok((regs[1], regs[2], regs[3]))
}
