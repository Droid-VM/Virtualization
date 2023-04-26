use super::pci::PCI_INFO;
use crate::memory::{alloc_shared, dealloc_shared, phys_to_virt, virt_to_phys};
use core::{
    alloc::Layout,
    ops::Range,
    ptr::{copy_nonoverlapping, NonNull},
};
use log::debug;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

pub struct HalImpl;

/// # Safety
///
/// See the 'Implementation Safety' comments on methods below for how they fulfill the safety
/// requirements of the unsafe `Hal` trait.
unsafe impl Hal for HalImpl {
    /// # Implementation Safety
    ///
    /// `dma_alloc` ensures the returned DMA buffer is not aliased with any other allocation or
    /// reference in the program until it is deallocated by `dma_dealloc` by allocating a unique
    /// block of memory using `alloc_shared`, which is guaranteed to allocate valid and unique
    /// memory. We request an alignment of at least `PAGE_SIZE` from `alloc_shared`.
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        debug!("dma_alloc: pages={}", pages);
        let layout = Layout::from_size_align(pages * PAGE_SIZE, PAGE_SIZE);
        let vaddr =
            alloc_shared(layout).expect("Failed to allocate and share VirtIO DMA range with host");
        let paddr = virt_to_phys(vaddr);
        (paddr, vaddr)
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, vaddr: NonNull<u8>, pages: usize) -> i32 {
        debug!("dma_dealloc: paddr={:#x}, pages={}", paddr, pages);
        let layout = Layout::from_size_align(pages * PAGE_SIZE, PAGE_SIZE);
        // Safe because the memory was allocated by `dma_alloc` above using the same allocator, and
        // the layout is the same as was used then.
        unsafe {
            dealloc_shared(vaddr, layout).expect("Failed to unshare VirtIO DMA range with host");
        }
        0
    }

    /// # Implementation Safety
    ///
    /// The returned pointer must be valid because the `paddr` describes a valid MMIO region, we
    /// check that it is within the PCI MMIO range, and we previously mapped the entire PCI MMIO
    /// range. It can't alias any other allocations because we previously validated in
    /// `map_mmio_range` that the PCI MMIO range didn't overlap with any other memory ranges.
    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, size: usize) -> NonNull<u8> {
        let pci_info = PCI_INFO.get().expect("VirtIO HAL used before PCI_INFO was initialised");
        // Check that the region is within the PCI MMIO range that we read from the device tree. If
        // not, the host is probably trying to do something malicious.
        if !contains_range(
            &pci_info.bar_range,
            &(paddr.try_into().expect("PCI MMIO region start was outside of 32-bit address space")
                ..paddr
                    .checked_add(size)
                    .expect("PCI MMIO region end overflowed")
                    .try_into()
                    .expect("PCI MMIO region end was outside of 32-bit address space")),
        ) {
            panic!("PCI MMIO region was outside of expected BAR range.");
        }
        phys_to_virt(paddr)
    }

    unsafe fn share(buffer: NonNull<[u8]>, direction: BufferDirection) -> PhysAddr {
        let size = buffer.len();

        // TODO: Copy to a pre-shared region rather than allocating and sharing each time.
        // Allocate a range of pages, copy the buffer if necessary, and share the new range instead.
        let copy =
            alloc_shared(size).expect("Failed to allocate and share VirtIO buffer with host");
        if direction == BufferDirection::DriverToDevice {
            unsafe {
                copy_nonoverlapping(buffer.as_ptr() as *mut u8, copy.as_ptr(), size);
            }
        }
        virt_to_phys(copy)
    }

    unsafe fn unshare(paddr: PhysAddr, buffer: NonNull<[u8]>, direction: BufferDirection) {
        let vaddr = phys_to_virt(paddr);
        let size = buffer.len();
        if direction == BufferDirection::DeviceToDriver {
            debug!(
                "Copying VirtIO buffer back from {:#x} to {:#x}.",
                paddr,
                buffer.as_ptr() as *mut u8 as usize
            );
            unsafe {
                copy_nonoverlapping(vaddr.as_ptr(), buffer.as_ptr() as *mut u8, size);
            }
        }

        // Unshare and deallocate the shared copy of the buffer.
        debug!("Unsharing VirtIO buffer {:#x}", paddr);
        // Safe because the memory was allocated by `share` using `alloc_shared`, and the size is
        // the same as was used then.
        unsafe {
            dealloc_shared(vaddr, size).expect("Failed to unshare VirtIO buffer with host");
        }
    }
}

/// Returns true if `inner` is entirely contained within `outer`.
fn contains_range(outer: &Range<u32>, inner: &Range<u32>) -> bool {
    inner.start >= outer.start && inner.end <= outer.end
}
