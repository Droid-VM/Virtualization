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

use core::ffi::CStr;
use core::ops::Range;
use libfdt::Fdt;
use libfdt::FdtError;

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

pub struct NextStageConfig {
    /// Address of the injected "google,open-dice"-compatible reserved-memory node.
    pub bcc: u64,
    /// Size of the injected "google,open-dice"-compatible reserved-memory node.
    pub bcc_size: u64,
    /// Whether to define the /chosen/avf,new-instance empty property.
    pub new_instance: bool,
    /// Whether to define the /chosen/avf,strict-boot empty property.
    pub strict_boot: bool,
}

impl NextStageConfig {
    /// Modifies the input DT according to the fields of the configuration.
    pub fn apply_to(&self, fdt: &mut Fdt) -> libfdt::Result<()> {
        fdt.unpack()?;

        add_dice_node(fdt, self.bcc, self.bcc_size)?;

        let strict_boot = CStr::from_bytes_with_nul(b"avf,strict-boot\0").unwrap();
        set_or_clear_chosen_flag(fdt, strict_boot, self.strict_boot)?;

        let new_instance = CStr::from_bytes_with_nul(b"avf,new-instance\0").unwrap();
        set_or_clear_chosen_flag(fdt, new_instance, self.new_instance)?;

        fdt.pack()?;

        Ok(())
    }
}

/// Add a "google,open-dice"-compatible reserved-memory node to the tree.
fn add_dice_node(fdt: &mut Fdt, addr: u64, size: u64) -> libfdt::Result<()> {
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

    let reg = CStr::from_bytes_with_nul(b"reg\0").unwrap();
    dice.appendprop_addrrange(reg, addr, size)?;

    Ok(())
}

fn set_or_clear_chosen_flag(fdt: &mut Fdt, flag: &CStr, value: bool) -> libfdt::Result<()> {
    // Panics if the DT doesn't contain a /chosen node.
    let mut chosen = fdt.chosen_mut()?.unwrap();
    if value {
        chosen.setprop_empty(flag)?;
    } else {
        match chosen.delprop(flag) {
            Ok(()) | Err(FdtError::NotFound) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(())
}
