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

//! Wrappers around hypervisor back-ends.

use crate::helpers::SIZE_4KB;
use crate::hypervisor::{gunyah::GunyahHypervisor, kvm::KvmHypervisor};
use crate::memory::{share_range, shared_buffer_layout, unshare_range, virt_to_phys};
use crate::smccc;
use crate::smccc::hvc64;
use alloc::alloc::{alloc_zeroed, dealloc, handle_alloc_error};
use core::fmt;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;

mod gunyah;
mod kvm;

pub enum Error {
    /// Error in hypervisor-specific initialization
    HypInitFailed,
    /// init() was called more than once
    InvalidInitState,
    /// Unknown Hypervisor
    UnknownHypervisorUUID(Uuid),
    /// Failure to parse UUID string
    UuidParseFailure,
}

pub type Result<T> = core::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::HypInitFailed => write!(f, "Failed to initialize hypervisor settings"),
            Self::InvalidInitState => write!(f, "init() called more than once"),
            Self::UnknownHypervisorUUID(e) => write!(f, "Unrecognized hypervisor UUID {:?}", e),
            Self::UuidParseFailure => write!(f, "Unable to parse UUID string"),
        }
    }
}

/// Trait for VM to declare its MMIO regions to the hypervisor
pub trait MmioGuard {
    /// Return Protected Granule size in bytes
    fn mmio_guard_info(&self) -> smccc::Result<u64> {
        Ok(SIZE_4KB as u64)
    }

    /// Register to use mmio_guard APIs
    fn mmio_guard_enroll(&self) -> smccc::Result<()> {
        Ok(())
    }

    /// Register an addresss as MMIO
    fn mmio_guard_map(&self, _ipa: u64) -> smccc::Result<()> {
        Ok(())
    }

    /// De-register an addresss as MMIO
    fn mmio_guard_unmap(&self, _ipa: u64) -> smccc::Result<()> {
        Ok(())
    }
}

/// Trait for VM to share its memory with host
pub trait MemSharing {
    /// Shares a region of memory with host, granting it read, write and execute permissions.
    /// The size of the region is equal to the memory protection granule returned by [`hyp_meminfo`].
    fn mem_share(&self, base_ipa: u64) -> smccc::Result<()>;

    /// Revokes access permission from host to a memory region previously shared with
    /// [`mem_share`]. The size of the region is equal to the memory protection granule returned by
    /// [`hyp_meminfo`].
    fn mem_unshare(&self, base_ipa: u64) -> smccc::Result<()>;

    /// Allocates a memory range of at least the given size, and shares it
    /// with the host. Returns a pointer to the buffer.
    ///
    /// It will be aligned to the memory sharing granule size supported by the hypervisor.
    fn alloc_shared(&self, size: usize) -> smccc::Result<NonNull<u8>> {
        let layout = shared_buffer_layout(size)?;
        let granule = layout.align();

        // Safe because `shared_buffer_layout` panics if the size is 0, so the layout must have a
        // non-zero size.
        let buffer = unsafe { alloc_zeroed(layout) };

        let Some(buffer) = NonNull::new(buffer) else {
            handle_alloc_error(layout);
        };

        let paddr = virt_to_phys(buffer);
        // If share_range fails then we will leak the allocation, but that seems better than having it
        // be reused while maybe still partially shared with the host.
        share_range(&(paddr..paddr + layout.size()), granule)?;

        Ok(buffer)
    }

    /// Unshares and deallocates a memory range which was previously allocated by `alloc_shared`.
    ///
    /// The size passed in must be the size passed to the original `alloc_shared` call.
    ///
    /// # Safety
    ///
    /// The memory must have been allocated by `alloc_shared` with the same size, and not yet
    /// deallocated.
    unsafe fn dealloc_shared(&self, vaddr: NonNull<u8>, size: usize) -> smccc::Result<()> {
        let layout = shared_buffer_layout(size)?;
        let granule = layout.align();

        let paddr = virt_to_phys(vaddr);
        unshare_range(&(paddr..paddr + layout.size()), granule)?;
        // Safe because the memory was allocated by `alloc_shared` above using the same allocator, and
        // the layout is the same as was used then.
        unsafe { dealloc(vaddr.as_ptr(), layout) };

        Ok(())
    }
}

/// Trait to use hypervisor-specific APIs
pub trait Hypervisor: MmioGuard + MemSharing {
    /// Queries the memory protection parameters for a protected virtual machine.
    ///
    /// Returns the memory protection granule size in bytes.
    fn hyp_meminfo(&self) -> smccc::Result<u64> {
        Ok(SIZE_4KB as u64)
    }
}

const UNINITIALIZED: usize = 0;
const INITIALIZED: usize = 1;
static HYP_INIT_STATE: AtomicUsize = AtomicUsize::new(UNINITIALIZED);
static mut CUR_HYPERVISOR: Option<&dyn Hypervisor> = None;
const ARM_SMCCC_VENDOR_HYP_CALL_UID_FUNC_ID: u32 = 0x8600ff01;
const KVM_UUID: &str = "28b46fb6-2ec5-11e9-a9ca-4b564d003a74";
const GUNYAH_UUID: &str = "19bd54bd-0b37-571b-946f-609b54539de6";

fn get_hyp_uuid() -> Uuid {
    let args = [0u64; 17];
    let res = hvc64(ARM_SMCCC_VENDOR_HYP_CALL_UID_FUNC_ID, args);
    Uuid::from_u128(
        (res[0] as u128)
            | ((res[1] as u128) << 32)
            | ((res[2] as u128) << 64)
            | ((res[3] as u128) << 96),
    )
}

pub fn init(fdt: &libfdt::Fdt) -> Result<()> {
    let old_state = HYP_INIT_STATE.compare_exchange(
        UNINITIALIZED,
        INITIALIZED,
        Ordering::SeqCst,
        Ordering::SeqCst,
    );

    if old_state.is_ok() {
        let hyp_uuid = get_hyp_uuid();
        let kvm_uuid = Uuid::parse_str(KVM_UUID).map_err(|_| Error::UuidParseFailure)?;
        let gunyah_uuid = Uuid::parse_str(GUNYAH_UUID).map_err(|_| Error::UuidParseFailure)?;

        match hyp_uuid {
            o if o == kvm_uuid => {
                // safe as we let this code snippet execute only once
                unsafe {
                    CUR_HYPERVISOR = Some(&KvmHypervisor);
                }
                Ok(())
            }

            o if o == gunyah_uuid => {
                gunyah::init(fdt).map_err(|_| Error::HypInitFailed)?;
                // safe as we let this code snippet execute only once
                unsafe {
                    CUR_HYPERVISOR = Some(&GunyahHypervisor);
                }
                Ok(())
            }

            _ => Err(Error::UnknownHypervisorUUID(hyp_uuid)),
        }
    } else {
        Err(Error::InvalidInitState)
    }
}

fn get_cur_hyp() -> &'static dyn Hypervisor {
    // SAFETY - this is mutated only once as part of the initialization and
    // access to this happens only after the initialization
    unsafe { CUR_HYPERVISOR.unwrap() }
}

pub fn hyp_meminfo() -> smccc::Result<u64> {
    get_cur_hyp().hyp_meminfo()
}

pub fn mmio_guard_enroll() -> smccc::Result<()> {
    get_cur_hyp().mmio_guard_enroll()
}

pub fn mmio_guard_info() -> smccc::Result<u64> {
    get_cur_hyp().mmio_guard_info()
}

pub fn mmio_guard_map(ipa: u64) -> smccc::Result<()> {
    get_cur_hyp().mmio_guard_map(ipa)
}

pub fn mmio_guard_unmap(ipa: u64) -> smccc::Result<()> {
    get_cur_hyp().mmio_guard_unmap(ipa)
}

pub fn alloc_shared(size: usize) -> smccc::Result<NonNull<u8>> {
    get_cur_hyp().alloc_shared(size)
}

pub unsafe fn dealloc_shared(vaddr: NonNull<u8>, size: usize) -> smccc::Result<()> {
    get_cur_hyp().dealloc_shared(vaddr, size)
}

pub fn mem_share(base_ipa: u64) -> smccc::Result<()> {
    get_cur_hyp().mem_share(base_ipa)
}

pub fn mem_unshare(base_ipa: u64) -> smccc::Result<()> {
    get_cur_hyp().mem_unshare(base_ipa)
}
