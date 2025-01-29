use super::common::Hypervisor;
use uuid::{uuid, Uuid};
use super::{DeviceAssigningHypervisor, MemSharingHypervisor};
use crate::{Result, Error};
use thiserror::Error;

pub(super) struct GunyahHypervisor;

impl GunyahHypervisor {
    pub const UUID: Uuid = uuid!("c1d58fcd-a453-5fdb-9265-ce36673d5f14");
}

impl Hypervisor for GunyahHypervisor {
    fn as_mem_sharer(&self) -> Option<&dyn MemSharingHypervisor> {
        Some(self)
    }
    fn as_device_assigner(&self) -> Option<&dyn DeviceAssigningHypervisor> {
        Some(self)
    }
}

impl DeviceAssigningHypervisor for GunyahHypervisor {
    fn get_phys_mmio_token(&self, base_ipa: u64) -> Result<u64> {
        Ok(base_ipa)
    }

    fn get_phys_iommu_token(&self, _pviommu_id: u64, _vsid: u64) -> Result<(u64, u64)> {
        Err(Error::GunyahError(GunyahError::NotSupported))
    }
}

impl MemSharingHypervisor for GunyahHypervisor {
    fn share(&self, _base_ipa: u64) -> Result<()> {
        Err(Error::GunyahError(GunyahError::NotSupported))
    }

    fn unshare(&self, _base_ipa: u64) -> Result<()> {
        Err(Error::GunyahError(GunyahError::NotSupported))
    }

    fn granule(&self) -> Result<usize> {
        Ok(4096)
    }
}

/// Error from a Gunyah HVC call.
#[derive(Copy, Clone, Debug, Eq, Error, PartialEq)]
pub enum GunyahError {
    /// The call is not supported by the implementation.
    #[error("Gunyah call not supported")]
    NotSupported,
}
