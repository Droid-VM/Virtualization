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

//! High-level FDT functions.

use crate::RebootReason;
use crate::helpers::GUEST_PAGE_SIZE;
use core::ffi::CStr;
use core::ops::Range;
use libfdt::Fdt;
use libfdt::FdtError;
use log::error;

const RAM_BASE_ADDR: u64 = 0x8000_0000;

/// Extract from /config the address range containing the pre-loaded kernel.
pub fn kernel_range(fdt: &libfdt::Fdt) -> libfdt::Result<Option<Range<usize>>> {
    let config = CStr::from_bytes_with_nul(b"/config\0").unwrap();
    let addr = CStr::from_bytes_with_nul(b"kernel-address\0").unwrap();
    let size = CStr::from_bytes_with_nul(b"kernel-size\0").unwrap();

    if let Some(config) = fdt.node(config)? {
        if let (Some(addr), Some(size)) = (config.getprop_u32(addr)?, config.getprop_u32(size)?) {
            let addr = addr as usize;
            let size = size as usize;

            return Ok(Some(addr..(addr + size)));
        }
    }

    Ok(None)
}

/// Extract from /chosen the address range containing the pre-loaded ramdisk.
pub fn initrd_range(fdt: &libfdt::Fdt) -> libfdt::Result<Option<Range<usize>>> {
    let start = CStr::from_bytes_with_nul(b"linux,initrd-start\0").unwrap();
    let end = CStr::from_bytes_with_nul(b"linux,initrd-end\0").unwrap();

    if let Some(chosen) = fdt.chosen()? {
        if let (Some(start), Some(end)) = (chosen.getprop_u32(start)?, chosen.getprop_u32(end)?) {
            return Ok(Some((start as usize)..(end as usize)));
        }
    }

    Ok(None)
}

/// Read and validate the size and base address of memory, and returns the size
fn parse_memory_node(fdt: &libfdt::Fdt) -> Result<usize, RebootReason> {
    let memory_range = fdt.memory()
        // actually, these checks are unnecessary because we read /memory node in entry.rs
        // where the exact same checks are done. we are repeating the same check just for
        // extra safety (in case when the code structure changes in the future).
        .map_err(|e| {
            error!("Failed to get /memory from the DT: {e}");
            RebootReason::InvalidFdt
        })?
        .ok_or_else(|| {
            error!("Node /memory was found empty");
            RebootReason::InvalidFdt
        })?
        .next()
        .ok_or_else(|| {
            error!("Failed to read memory range from the DT");
            RebootReason::InvalidFdt
        })?;

    let base = memory_range.start;
    if base as u64 != RAM_BASE_ADDR {
        error!("Memory base address {:#x} is not {:#x}", base, RAM_BASE_ADDR);
        return Err(RebootReason::InvalidFdt);
    }

    let size = memory_range.end - memory_range.start;
    if size % GUEST_PAGE_SIZE != 0 {
        error!("Memory size {:#x} is not a multiple of page size {:#x}", size, GUEST_PAGE_SIZE);
        return Err(RebootReason::InvalidFdt);
    }
    // In the u-boot implementation, we checked if base + size > u64::MAX, but we don't need that
    // because memory() function uses checked_add when constructing the Range object. If an
    // overflow happened, we should have gotten None from the next() call above and would have
    // bailed already.

    Ok(size)
}

/// Read the number of CPUs
fn parse_cpu_nodes(fdt: &libfdt::Fdt) -> Result<usize, RebootReason> {
    Ok(fdt.compatible_nodes(CStr::from_bytes_with_nul(b"arm,arm-v8\0").unwrap())
        .map_err(|e| {
            error!("Failed to read compatible nodes \"arm,arm-v8\" from DT: {e}");
            RebootReason::InvalidFdt
        })?
        .count())
}

struct PciInfo {
    low_addr: u64,
    low_size: u64,
    high_addr: u64,
    high_size: u64,
    num_irq: usize,
}

/// Read and validate PCI node
fn parse_pci_nodes(fdt: &libfdt::Fdt) -> Result<PciInfo, RebootReason> {
    let node = fdt.compatible_nodes(CStr::from_bytes_with_nul(b"pci-host-cam-generic\0").unwrap())
        .map_err(|e| {
            error!("Failed to read compatible node \"pci-host-cam-generic\" from DT: {e}");
            RebootReason::InvalidFdt
        })?
        .next()
        .ok_or_else(|| {
            error("Compatible node \"pci-host-cam-generic\" doesn't exist");
            RebootReason::InvalidFdt // why should this be an error? a VM without any pci is
                                     // possible?
        })?;

    let iter = node.ranges::<(u32, u64), u64, u64>()
        .map_err(|e| {
            error!("Failed to read ranges from PCI node: {e}");
            RebootReason::InvalidFdt
        })?
        .ok_or_else(|| {
            error!("PCI node missing ranges property");
            RebootReason::InvalidFdt
        })?;

    let range_low = iter.next().ok_or_else(|| {
        error!("Low range missing in PCI node");
        RebootReason
    })?;
    validate_pci_range(&range_low)?;

    let range_high = iter.next().ok_or_else(|| {
        error!("High range missing in PCI node");
        RebootReason
    })?;
    validate_pci_range(&range_high)?;


}

fn validate_pci_range(range: &AddressRange<(u32, u64), u64, u64>) -> Result<(), RebootReason> {
    let range_type = range.addr.0.range_type();
    let bus_addr = range.addr.1;
    let cpu_addr = range.parent;
    let size = range.size;
    if range_type != PciRangeType::Memory64 {
        error!("Invalid range type {:?} in PCI node", range_type);
        return Err(InvalidFdt);
    }
    if bus_addr != cpu_addr {
        error!("PCI bus address: {:#x} is different from CPU address: {:#x}", bus_addr, cpu_addr);
        return Err(InvalidFdt);
    }
    if bus_addr.checked_add(size).is_none() {
        error!("PCI address range size {:#x} too big", size);
        return Err(InvalidFdt);
    }
    Ok(())
}

/// Iterator over N cells as a chunk
struct CellChunkIterator<'a> {
    cells: CellIterator<'a>,
    num_cells: usize,
}

impl<'a> CellChunkIterator<'a> {
    fn new(cells: CellIterator<'a>, num_cells: usize) -> Self {
        Self { cells, num_cells }
    }
}

impl<'a> Iterator for CellChunkIterator<'a> {
    type Item = [u32];
    fn next(&mut self) -> Option<Self::Item> {

    }
}

fn count_and_validate_pci_irq_masks(pciNode: &libfdt::FdtNode) -> Result<usize, RebootReason> {
    let name = CStr::from_bytes_with_nul(b"interrupt-map-mask\0").unwrap();
    let count = pciNode.getprop_cells(&name).count();

    pciNode.getprop_cells
    let count = pciNode.getprop_cells(Cstr::from_bytes_with_nul(b"interrupt-map-mask\0").unwrap()).count();

    let node = fdt.compatible_nodes(CStr::from_bytes_with_nul(b"pci-host-cam-generic\0").unwrap())
}

/// Modifies the input DT according to the fields of the configuration.
pub fn modify_for_next_stage(
    fdt: &mut Fdt,
    bcc: &[u8],
    new_instance: bool,
    strict_boot: bool,
) -> libfdt::Result<()> {
    fdt.unpack()?;

    add_dice_node(fdt, bcc.as_ptr() as usize, bcc.len())?;

    set_or_clear_chosen_flag(
        fdt,
        CStr::from_bytes_with_nul(b"avf,strict-boot\0").unwrap(),
        strict_boot,
    )?;
    set_or_clear_chosen_flag(
        fdt,
        CStr::from_bytes_with_nul(b"avf,new-instance\0").unwrap(),
        new_instance,
    )?;

    fdt.pack()?;

    Ok(())
}

/// Add a "google,open-dice"-compatible reserved-memory node to the tree.
fn add_dice_node(fdt: &mut Fdt, addr: usize, size: usize) -> libfdt::Result<()> {
    let reserved_memory = CStr::from_bytes_with_nul(b"/reserved-memory\0").unwrap();
    // We reject DTs with missing reserved-memory node as validation should have checked that the
    // "swiotlb" subnode (compatible = "restricted-dma-pool") was present.
    let mut reserved_memory = fdt.node_mut(reserved_memory)?.ok_or(libfdt::FdtError::NotFound)?;

    let dice = CStr::from_bytes_with_nul(b"dice\0").unwrap();
    let mut dice = reserved_memory.add_subnode(dice)?;

    let compatible = CStr::from_bytes_with_nul(b"compatible\0").unwrap();
    dice.appendprop(compatible, b"google,open-dice\0")?;

    let no_map = CStr::from_bytes_with_nul(b"no-map\0").unwrap();
    dice.appendprop(no_map, &[])?;

    let addr = addr.try_into().unwrap();
    let size = size.try_into().unwrap();
    let reg = CStr::from_bytes_with_nul(b"reg\0").unwrap();
    dice.appendprop_addrrange(reg, addr, size)?;

    Ok(())
}

fn set_or_clear_chosen_flag(fdt: &mut Fdt, flag: &CStr, value: bool) -> libfdt::Result<()> {
    // TODO(b/249054080): Refactor to not panic if the DT doesn't contain a /chosen node.
    let mut chosen = fdt.chosen_mut()?.unwrap();
    if value {
        chosen.setprop_empty(flag)?;
    } else {
        match chosen.delprop(flag) {
            Ok(()) | Err(FdtError::NotFound) => (),
            Err(e) => return Err(e),
        }
    }

    Ok(())
}
