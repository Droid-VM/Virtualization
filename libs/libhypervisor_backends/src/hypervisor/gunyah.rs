use super::common::Hypervisor;
use uuid::{uuid, Uuid};
use super::DeviceAssigningHypervisor;
use crate::{Result, Error};
use thiserror::Error;
use smccc::{
    error::{success_or_error_64},
    hvc64,
};

const VENDOR_HYP_GUNYAH_DEV_REQ_MMIO_FUNC_ID: u32 = 0xc6000012;
const VENDOR_HYP_GUNYAH_DEV_REQ_DMA_FUNC_ID: u32 = 0xc600001b;

pub(super) struct GunyahHypervisor;

/// Error from a Gunyah HVC call.
#[derive(Copy, Clone, Debug, Eq, Error, PartialEq)]
pub enum GunyahError {
    /// The call is not supported by the implementation.
    #[error("Gunyah call not supported")]
    NotSupported,
    /// One of the call parameters has a invalid value.
    #[error("Gunyah call received invalid value")]
    InvalidParameter,
    /// There was an unexpected return value.
    #[error("Unknown return value from Gunyah {0} ({0:#x})")]
    Unknown(i64),
}

impl From<i64> for GunyahError {
    fn from(value: i64) -> Self {
        match value {
            -1 => GunyahError::NotSupported,
            -3 => GunyahError::InvalidParameter,
            _ => GunyahError::Unknown(value),
        }
    }
}

impl From<i32> for GunyahError {
    fn from(value: i32) -> Self {
        i64::from(value).into()
    }
}

impl GunyahHypervisor {
    pub const UUID: Uuid = uuid!("c1d58fcd-a453-5fdb-9265-ce36673d5f14");
}

impl Hypervisor for GunyahHypervisor {
    fn as_device_assigner(&self) -> Option<&dyn DeviceAssigningHypervisor> {
        Some(self)
    }
}

impl DeviceAssigningHypervisor for GunyahHypervisor {
    fn get_phys_mmio_token(&self, base_ipa: u64) -> Result<u64> {
        let mut args = [0u64; 17];
        args[0] = base_ipa;

        match checked_hvc64_expect_results(VENDOR_HYP_GUNYAH_DEV_REQ_MMIO_FUNC_ID, args) {
            Ok(ret) => Ok(ret[0]),
            Err(Error::GunyahError(GunyahError::NotSupported, _)) => Ok(base_ipa),
            Err(e) => Err(e),
        }
    }

    fn get_phys_iommu_token(&self, pviommu_id: u64, vsid: u64) -> Result<(u64, u64)> {
        let mut args = [0u64; 17];
        args[0] = pviommu_id;
        args[1] = vsid;

        let ret = checked_hvc64_expect_results(VENDOR_HYP_GUNYAH_DEV_REQ_DMA_FUNC_ID, args)?;
        Ok((ret[0], ret[1]))
    }
}

fn checked_hvc64_expect_results(function: u32, args: [u64; 17]) -> Result<[u64; 17]> {
    let [ret, results @ ..] = hvc64(function, args);
    success_or_error_64(ret).map_err(|e| Error::GunyahError(e, function))?;
    Ok(results)
}
