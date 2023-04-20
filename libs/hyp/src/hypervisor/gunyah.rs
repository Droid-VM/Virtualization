use crate::error::{Result};
use crate::Hypervisor;
use crate::util::SIZE_4KB;
use uuid::{uuid, Uuid};

pub(super) struct GunyahHypervisor;

impl GunyahHypervisor {
    pub const UUID: Uuid = uuid!("145f3d67-36ce-6592-db5f-53a4cd8fd5c1");
}

impl Hypervisor for GunyahHypervisor {
    fn mmio_guard_init(&self) -> Result<()> {
        Ok(())
    }

    fn mmio_guard_map(&self, _addr: usize) -> Result<()> {
        Ok(())
    }

    fn mmio_guard_unmap(&self, _addr: usize) -> Result<()> {
        Ok(())
    }

    fn mem_share(&self, _base_ipa: u64) -> Result<()> {
        unimplemented!();
    }

    fn mem_unshare(&self, _base_ipa: u64) -> Result<()> {
        unimplemented!();
    }

    fn memory_protection_granule(&self) -> Result<usize> {
        Ok(SIZE_4KB)
    }

    fn has_cap(&self, _cap: u32) -> bool {
        false
    }
}
