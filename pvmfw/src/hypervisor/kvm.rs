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

use crate::hvc;
use crate::hypervisor::{Hypervisor, MemSharing, MmioGuard};
use crate::smccc;

pub struct KvmHypervisor;

impl MmioGuard for KvmHypervisor {
    fn mmio_guard_info(&self) -> smccc::Result<u64> {
        hvc::kvm_mmio_guard_info()
    }

    fn mmio_guard_enroll(&self) -> smccc::Result<()> {
        hvc::kvm_mmio_guard_enroll()
    }

    fn mmio_guard_map(&self, ipa: u64) -> smccc::Result<()> {
        hvc::kvm_mmio_guard_map(ipa)
    }

    fn mmio_guard_unmap(&self, ipa: u64) -> smccc::Result<()> {
        hvc::kvm_mmio_guard_unmap(ipa)
    }
}

impl MemSharing for KvmHypervisor {
    fn mem_share(&self, base_ipa: u64) -> smccc::Result<()> {
        hvc::kvm_mem_share(base_ipa)
    }

    fn mem_unshare(&self, base_ipa: u64) -> smccc::Result<()> {
        hvc::kvm_mem_unshare(base_ipa)
    }
}

impl Hypervisor for KvmHypervisor {
    fn hyp_meminfo(&self) -> smccc::Result<u64> {
        hvc::kvm_hyp_meminfo()
    }
}
