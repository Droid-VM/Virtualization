use super::common::{Hypervisor, HypervisorCap, UniqueID};
use crate::util::SIZE_4KB;
use smccc::Result;

pub(super) struct GunyahHypervisor;

const GUNYAH_UUID: u128 = 0x145f3d67_36ce6592_db5f53a4_cd8fd5c1;
const GUNYAH_NAME: &str = "Gunyah";

impl UniqueID for GunyahHypervisor {
    const UUID: u128 = GUNYAH_UUID;
}

impl Hypervisor for GunyahHypervisor {
    fn mmio_guard_info(&self) -> Result<u64> {
        unimplemented!();
    }

    fn mmio_guard_enroll(&self) -> Result<()> {
        unimplemented!();
    }

    fn mmio_guard_map(&self, _ipa: u64) -> Result<()> {
        unimplemented!();
    }

    fn mmio_guard_unmap(&self, _ipa: u64) -> Result<()> {
        unimplemented!();
    }

    fn mem_share(&self, _base_ipa: u64) -> Result<()> {
        unimplemented!();
    }

    fn mem_unshare(&self, _base_ipa: u64) -> Result<()> {
        unimplemented!();
    }

    fn hyp_meminfo(&self) -> Result<u64> {
        Ok(SIZE_4KB.try_into().unwrap())
    }

    fn check_capability(&self, cap: HypervisorCap) -> bool {
        match cap {
            HypervisorCap::MemShare => false,
            HypervisorCap::MmioGuard => false,
        }
    }

    fn name(&self) -> &'static str {
        GUNYAH_NAME
    }
}
