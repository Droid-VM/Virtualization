use crate::error::{Error, Result};
use super::common::{Hypervisor, HypervisorCap};
use core::fmt::{self, Display, Formatter};
use crate::util::{page_address, SIZE_4KB};
use uuid::{uuid, Uuid};
use psci::smccc::{
    error::{positive_or_error_64, success_or_error_32, success_or_error_64},
    hvc64,
};

pub(super) struct GeniezoneHypervisor;

const ARM_SMCCC_KVM_FUNC_HYP_MEMINFO: u32 = 0xc6000002;
const ARM_SMCCC_KVM_FUNC_MEM_SHARE: u32 = 0xc6000003;
const ARM_SMCCC_KVM_FUNC_MEM_UNSHARE: u32 = 0xc6000004;

const VENDOR_HYP_KVM_MMIO_GUARD_INFO_FUNC_ID: u32 = 0xc6000005;
const VENDOR_HYP_KVM_MMIO_GUARD_ENROLL_FUNC_ID: u32 = 0xc6000006;
const VENDOR_HYP_KVM_MMIO_GUARD_MAP_FUNC_ID: u32 = 0xc6000007;
const VENDOR_HYP_KVM_MMIO_GUARD_UNMAP_FUNC_ID: u32 = 0xc6000008;

impl GeniezoneHypervisor {
    pub const UUID: Uuid = uuid!("ba8fd0d9-4087-4f8f-a1e4-b45c580812a1");
    const CAPABILITIES: HypervisorCap = HypervisorCap::DYNAMIC_MEM_SHARE;
}

/// Error from a Geniezone HVC call.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum GeniezoneError {
    /// The call is not supported by the implementation.
    NotSupported,
    /// The call is not required to implement.
    NotRequired,
    /// One of the call parameters has a invalid value.
    InvalidParameter,
    /// There was an unexpected return value.
    Unknown(i64),
}

impl From<i64> for GeniezoneError {
    fn from(value: i64) -> Self {
        match value {
            -1 => GeniezoneError::NotSupported,
            -2 => GeniezoneError::NotRequired,
            -3 => GeniezoneError::InvalidParameter,
            _ => GeniezoneError::Unknown(value),
        }
    }
}

impl From<i32> for GeniezoneError {
    fn from(value: i32) -> Self {
        i64::from(value).into()
    }
}

impl Display for GeniezoneError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::NotSupported => write!(f, "Geniezone call not supported"),
            Self::NotRequired => write!(f, "Geniezone call not required"),
            Self::InvalidParameter => write!(f, "Geniezone call received invalid value"),
            Self::Unknown(e) => write!(f, "Unknown return value from KVM {} ({0:#x})", e),
        }
    }
}

impl Hypervisor for GeniezoneHypervisor{
    fn mmio_guard_init(&self) -> Result<()> {
        mmio_guard_enroll()?;
        let mmio_granule = mmio_guard_granule()?;
        if mmio_granule != SIZE_4KB {
            return Err(Error::UnsupportedMmioGuardGranule(mmio_granule));
        }
        Ok(())
    }

    fn mmio_guard_map(&self, addr: usize) -> Result<()> {
        let mut args = [0u64; 17];
        args[0] = page_address(addr);

        success_or_error_32(hvc64(VENDOR_HYP_KVM_MMIO_GUARD_MAP_FUNC_ID, args)[0] as u32)
            .map_err(|e| Error::GeniezoneError(e, VENDOR_HYP_KVM_MMIO_GUARD_MAP_FUNC_ID))
    }

    fn mmio_guard_unmap(&self, addr: usize) -> Result<()> {
        let mut args = [0u64; 17];
        args[0] = page_address(addr);

        match success_or_error_64(hvc64(VENDOR_HYP_KVM_MMIO_GUARD_UNMAP_FUNC_ID, args)[0]) {
            Err(GeniezoneError::NotSupported) | Err(GeniezoneError::NotRequired) | Ok(_) => Ok(()),
            Err(e) => Err(Error::GeniezoneError(e, VENDOR_HYP_KVM_MMIO_GUARD_UNMAP_FUNC_ID)),
        }
    }

    fn mem_share(&self, base_ipa: u64) -> Result<()> {
        let mut args = [0u64; 17];
        args[0] = base_ipa;

        checked_hvc64_expect_zero(ARM_SMCCC_KVM_FUNC_MEM_SHARE, args)
    }

    fn mem_unshare(&self, base_ipa: u64) -> Result<()> {
        let mut args = [0u64; 17];
        args[0] = base_ipa;

        checked_hvc64_expect_zero(ARM_SMCCC_KVM_FUNC_MEM_UNSHARE, args)
    }

    fn memory_protection_granule(&self) -> Result<usize> {
        let args = [0u64; 17];
        let granule = checked_hvc64(ARM_SMCCC_KVM_FUNC_HYP_MEMINFO, args)?;
        Ok(granule.try_into().unwrap())
    }

    fn has_cap(&self, cap: HypervisorCap) -> bool {
        Self::CAPABILITIES.contains(cap)
    }
}

fn mmio_guard_granule() -> Result<usize> {
    let args = [0u64; 17];

    let granule = checked_hvc64(VENDOR_HYP_KVM_MMIO_GUARD_INFO_FUNC_ID, args)?;
    Ok(granule.try_into().unwrap())
}

fn mmio_guard_enroll() -> Result<()> {
    let args = [0u64; 17];
    match success_or_error_64(hvc64(VENDOR_HYP_KVM_MMIO_GUARD_ENROLL_FUNC_ID, args)[0]) {
        Ok(_) => Ok(()),
        Err(GeniezoneError::NotSupported) => Err(Error::MmioGuardNotsupported),
        Err(GeniezoneError::NotRequired) => Err(Error::MmioGuardNotsupported),
        Err(e) => Err(Error::GeniezoneError(e, VENDOR_HYP_KVM_MMIO_GUARD_ENROLL_FUNC_ID)),
    }
}

fn checked_hvc64_expect_zero(function: u32, args: [u64; 17]) -> Result<()> {
    success_or_error_64(hvc64(function, args)[0]).map_err(|e| Error::GeniezoneError(e, function))
}

fn checked_hvc64(function: u32, args: [u64; 17]) -> Result<u64> {
    positive_or_error_64(hvc64(function, args)[0]).map_err(|e| Error::GeniezoneError(e, function))
}
