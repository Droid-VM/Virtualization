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

//! Validate device assignment written in crosvm DT with VM DTBO, and apply it
//! to platform DT.
//! Declared in separated libs for adding unit tests, which requires libstd.

#[cfg(test)]
extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::ffi::CString;
use alloc::fmt;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::CStr;
use core::iter::Iterator;
use core::mem;
use libfdt::{Fdt, FdtError, FdtNode, Phandle};

// TODO(b/308694211): Use cstr! from vmbase instead.
macro_rules! cstr {
    ($str:literal) => {{
        const S: &str = concat!($str, "\0");
        const C: &::core::ffi::CStr = match ::core::ffi::CStr::from_bytes_with_nul(S.as_bytes()) {
            Ok(v) => v,
            Err(_) => panic!("string contains interior NUL"),
        };
        C
    }};
}

// TODO(b/277993056): Keep constants derived from platform.dts in one place.
const CELLS_PER_INTERRUPT: usize = 3; // from /intc node in platform.dts

/// Errors in device assignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceAssignmentError {
    // Invalid VM DTBO
    InvalidDtbo,
    /// Invalid __symbols__
    InvalidSymbols,
    /// Invalid <interrupts>
    InvalidInterrupts,
    /// Invalid <iommus>
    InvalidIommus,
    /// Too many PV IOMMU.
    TooManyPvIommu,
    /// Unsupported overlay target syntax. Only supports <target-path> with full path.
    UnsupportedOverlayTarget,
    /// Unexpected error from libfdt
    UnexpectedFdtError(FdtError),
}

impl From<FdtError> for DeviceAssignmentError {
    fn from(e: FdtError) -> Self {
        DeviceAssignmentError::UnexpectedFdtError(e)
    }
}

impl fmt::Display for DeviceAssignmentError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidDtbo => write!(f, "Invalid DTBO"),
            Self::InvalidSymbols => write!(
                f,
                "Invalid property in /__symbols__. Must point to valid assignable device node."
            ),
            Self::InvalidInterrupts => write!(f, "Invalid <interrupts>"),
            Self::InvalidIommus => write!(f, "Invalid <iommus>"),
            Self::TooManyPvIommu => write!(
                f,
                "Too many PV IOMMU node. Insufficient pre-populated PV IOMMUs in platform DT"
            ),
            Self::UnsupportedOverlayTarget => {
                write!(f, "Unsupported overlay target. Only supports 'target-path = \"/\"'")
            }
            Self::UnexpectedFdtError(e) => write!(f, "Unexpected Error from libfdt: {e}"),
        }
    }
}

pub type Result<T> = core::result::Result<T, DeviceAssignmentError>;

/// Represents VM DTBO
#[repr(transparent)]
pub struct VmDtbo(Fdt);

impl VmDtbo {
    /// Wraps a mutable slice containing a VM DTBO.
    ///
    /// Fails if the VM DTBO does not pass validation.
    pub fn from_mut_slice(dtbo: &mut [u8]) -> Result<&mut Self> {
        // This validates DTBO
        let fdt = Fdt::from_mut_slice(dtbo)?;
        // SAFETY: VmDtbo is a transparent wrapper around Fdt, so representation is the same.
        Ok(unsafe { mem::transmute::<&mut Fdt, &mut Self>(fdt) })
    }

    // Locates device node path as if the given dtbo node path is assigned and VM DTBO is overlaid.
    // For given dtbo node path, this concatenates <target-path> of the enclosing fragment and
    // relative path from __overlay__ node.
    //
    // Here's an example with sample VM DTBO:
    //    / {
    //       fragment@rng {
    //         target-path = "/";  // Always 'target-path = "/"'. Disallows <target> or other path.
    //         __overlay__ {
    //           rng { ... };      // Actual device node is here. If overlaid, path would be "/rng"
    //         };
    //       };
    //       __symbols__ {         // List of assignable devices
    //         // Each property describes an assigned device device information.
    //         // property name is the device label, and property value is the path in the VM DTBO.
    //         rng = "/fragment@rng/__overlay__/rng";
    //       };
    //    };
    //
    // Then locate_overlay_target_path(cstr!("/fragment@rng/__overlay__/rng")) is Ok("/rng")
    //
    // Contrary to fdt_overlay_target_offset(), this API enforces overlay target property
    // 'target-path = "/"', so the overlay doesn't modify and/or append platform DT's existing
    // node and/or properties. The enforcement is for compatibility reason.
    fn locate_overlay_target_path(&self, dtbo_node_path: &CStr) -> Result<CString> {
        let dtbo_node_path_bytes = dtbo_node_path.to_bytes();
        if dtbo_node_path_bytes.first() != Some(&b'/') {
            return Err(DeviceAssignmentError::UnsupportedOverlayTarget);
        }

        let node = self.0.node(dtbo_node_path)?.ok_or(DeviceAssignmentError::InvalidSymbols)?;

        let fragment_node = node.supernode_at_depth(1)?;
        let target_path = fragment_node
            .getprop_str(cstr!("target-path"))?
            .ok_or(DeviceAssignmentError::InvalidDtbo)?;
        if target_path != cstr!("/") {
            return Err(DeviceAssignmentError::UnsupportedOverlayTarget);
        }

        let mut components = dtbo_node_path_bytes
            .split(|char| *char == b'/')
            .filter(|&component| !component.is_empty())
            .skip(1);
        let overlay_node_name = components.next();
        if overlay_node_name != Some(b"__overlay__") {
            return Err(DeviceAssignmentError::InvalidDtbo);
        }
        let mut overlaid_path = Vec::with_capacity(dtbo_node_path_bytes.len());
        for component in components {
            overlaid_path.push(b'/');
            overlaid_path.extend_from_slice(component);
        }
        overlaid_path.push(b'\0');

        Ok(CString::from_vec_with_nul(overlaid_path).unwrap())
    }
}

impl AsRef<Fdt> for VmDtbo {
    fn as_ref(&self) -> &Fdt {
        &self.0
    }
}

impl AsMut<Fdt> for VmDtbo {
    fn as_mut(&mut self) -> &mut Fdt {
        &mut self.0
    }
}

// PV IOMMU node's id
#[repr(transparent)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PvIommuId(u32);

impl From<u32> for PvIommuId {
    fn from(id: u32) -> PvIommuId {
        PvIommuId(id)
    }
}

impl From<PvIommuId> for u32 {
    fn from(id: PvIommuId) -> u32 {
        id.0
    }
}

// TODO(b/277993056): Also keeps vSID to handle #iommu-cells = <1>
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct PvIommu {
    // ID from pviommu node
    id: PvIommuId,
}

impl PvIommu {
    fn parse(node: &FdtNode) -> Result<Self> {
        let id = node.getprop_u32(cstr!("id"))?.ok_or(DeviceAssignmentError::InvalidIommus)?;
        Ok(Self { id: PvIommuId::from(id) })
    }
}

/// Assigned device information parsed from crosvm DT.
/// Keeps everything in the owned data because underlying FDT will be reused for platform DT.
#[derive(Debug, Eq, PartialEq)]
struct AssignedDeviceInfo {
    // Node path of assigned device (e.g. "/rng")
    node_path: CString,
    // DTBO node path of the assigned device (e.g. "/fragment@rng/__overlay__/rng")
    dtbo_node_path: CString,
    // <reg> property from the crosvm DT
    reg: Vec<u8>,
    // <interrupts> property from the crosvm DT
    interrupts: Vec<u8>,
    // Parsed <iommus> property from the crosvm DT.
    iommu_ids: Vec<PvIommuId>,
}

impl AssignedDeviceInfo {
    fn parse_interrupts(node: &FdtNode) -> Result<Vec<u8>> {
        // Validation: Validate if interrupts cell numbers are multiple of #interrupt-cells.
        // We can't know how many interrupts would exist.
        let interrupts_cells = node
            .getprop_cells(cstr!("interrupts"))?
            .ok_or(DeviceAssignmentError::InvalidInterrupts)?
            .count();
        if interrupts_cells % CELLS_PER_INTERRUPT != 0 {
            return Err(DeviceAssignmentError::InvalidInterrupts);
        }

        // Once validated, keep the raw bytes so patch can be done with setprop()
        Ok(node.getprop(cstr!("interrupts")).unwrap().unwrap().into())
    }

    fn parse_iommus(
        node: &FdtNode,
        pviommu_ids: &BTreeMap<Phandle, PvIommuId>,
    ) -> Result<Vec<PvIommuId>> {
        let mut pviommus = vec![];
        let Some(pviommu_phandles) = node.getprop_cells(cstr!("iommus"))? else {
            return Ok(pviommus);
        };
        for phandle in pviommu_phandles {
            let phandle =
                Phandle::try_from(phandle).or(Err(DeviceAssignmentError::InvalidIommus))?;
            let pviommu_id =
                pviommu_ids.get(&phandle).ok_or(DeviceAssignmentError::InvalidIommus)?;
            pviommus.push(*pviommu_id)
        }
        Ok(pviommus)
    }

    fn parse(
        fdt: &Fdt,
        vm_dtbo: &VmDtbo,
        dtbo_node_path: &CStr,
        pviommu_ids: &BTreeMap<Phandle, PvIommuId>,
    ) -> Result<Option<Self>> {
        let node_path = vm_dtbo.locate_overlay_target_path(dtbo_node_path)?;

        let Some(node) = fdt.node(&node_path)? else { return Ok(None) };

        // TODO(b/277993056): Validate reg with HVC, and keep reg with FdtNode::reg()
        let reg = node.getprop(cstr!("reg")).unwrap().unwrap();

        let interrupts = Self::parse_interrupts(&node)?;
        let iommu_ids = Self::parse_iommus(&node, pviommu_ids)?;
        Ok(Some(Self {
            node_path,
            dtbo_node_path: dtbo_node_path.into(),
            reg: reg.to_vec(),
            interrupts,
            iommu_ids,
        }))
    }

    fn patch(&self, fdt: &mut Fdt, pviommu_phandles: &BTreeMap<PvIommuId, Phandle>) -> Result<()> {
        let mut dst = fdt.node_mut(&self.node_path)?.unwrap();
        dst.setprop(cstr!("reg"), &self.reg)?;
        dst.setprop(cstr!("interrupts"), &self.interrupts)?;

        let iommus: Vec<u8> = self
            .iommu_ids
            .iter()
            .flat_map(|pviommu_id| {
                let phandle = pviommu_phandles.get(pviommu_id).unwrap();
                u32::from(*phandle).to_be_bytes()
            })
            .collect();
        dst.setprop(cstr!("iommus"), &iommus)?;

        Ok(())
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct DeviceAssignmentInfo {
    assigned_pviommus: BTreeMap<PvIommuId, PvIommu>,
    assigned_devices: Vec<AssignedDeviceInfo>,
    filtered_dtbo_paths: Vec<CString>,
}

impl DeviceAssignmentInfo {
    const PVIOMMU_COMPATIBLE: &CStr = cstr!("pkvm,pviommu");

    /// Parses fdt and vm_dtbo, and creates new DeviceAssignmentInfo
    // TODO(b/277993056): Parse __local_fixups__
    // TODO(b/277993056): Parse __fixups__
    pub fn parse(fdt: &Fdt, vm_dtbo: &VmDtbo) -> Result<Option<Self>> {
        let Some(symbols_node) = vm_dtbo.as_ref().symbols()? else {
            // /__symbols__ should contain all assignable devices.
            // If empty, then nothing can be assigned.
            return Ok(None);
        };

        // Parses all PV IOMMUs in fdt
        // Note: This will validate PV IOMMU ids' uniqueness, even when unassigned.
        let mut pviommus = BTreeMap::new();
        let mut pviommus_phandles = BTreeMap::new();
        for compatible in fdt.compatible_nodes(Self::PVIOMMU_COMPATIBLE)? {
            let pviommu = PvIommu::parse(&compatible)?;
            match pviommus.insert(pviommu.id, pviommu) {
                Some(_) => Err(DeviceAssignmentError::InvalidIommus), // id must be unique
                None => Ok(()),
            }?;
            let phandle = compatible.get_phandle()?.ok_or(DeviceAssignmentError::InvalidIommus)?;
            pviommus_phandles.insert(phandle, pviommu.id);
        }

        // Parses assigned devices
        let mut assigned_devices = vec![];
        let mut filtered_dtbo_paths = vec![];
        for symbol_prop in symbols_node.properties()? {
            let symbol_prop_value = symbol_prop.value()?;
            let dtbo_node_path = CStr::from_bytes_with_nul(symbol_prop_value)
                .or(Err(DeviceAssignmentError::InvalidSymbols))?;
            let assigned_device =
                AssignedDeviceInfo::parse(fdt, vm_dtbo, dtbo_node_path, &pviommus_phandles)?;
            if let Some(assigned_device) = assigned_device {
                assigned_devices.push(assigned_device);
            } else {
                filtered_dtbo_paths.push(dtbo_node_path.into());
            }
        }
        if assigned_devices.is_empty() {
            return Ok(None);
        }

        // Filters unused PV IOMMUs
        let mut assigned_pviommus: BTreeMap<PvIommuId, PvIommu> = BTreeMap::new();
        for assigned_device in &assigned_devices {
            for pviommu_id in &assigned_device.iommu_ids {
                let pviommu =
                    pviommus.get(pviommu_id).ok_or(DeviceAssignmentError::InvalidIommus)?;
                assigned_pviommus.insert(*pviommu_id, *pviommu);
            }
        }
        filtered_dtbo_paths.push(CString::new("/__symbols__").unwrap());

        Ok(Some(Self { assigned_pviommus, assigned_devices, filtered_dtbo_paths }))
    }

    /// Filters VM DTBO to only contain necessary information for booting pVM
    /// In detail, this will remove followings by setting nop node / nop property.
    ///   - Removes unassigned devices
    ///   - Removes /__symbols__ node
    // TODO(b/277993056): remove unused dependencies in VM DTBO.
    // TODO(b/277993056): remove supernodes' properties.
    // TODO(b/277993056): remove unused alises.
    pub fn filter(&self, vm_dtbo: &mut VmDtbo) -> Result<()> {
        let vm_dtbo = vm_dtbo.as_mut();

        // Filters unused node in assigned devices
        for filtered_dtbo_path in &self.filtered_dtbo_paths {
            let node = vm_dtbo.node_mut(filtered_dtbo_path).unwrap().unwrap();
            node.nop()?;
        }

        // Filters pvmfw-specific properties in assigned device node.
        const FILTERED_VM_DTBO_PROP: [&CStr; 3] = [
            cstr!("android,pvmfw,phy-reg"),
            cstr!("android,pvmfw,phy-iommu"),
            cstr!("android,pvmfw,phy-sid"),
        ];
        for assigned_device in &self.assigned_devices {
            let mut node = vm_dtbo.node_mut(&assigned_device.dtbo_node_path).unwrap().unwrap();
            for prop in FILTERED_VM_DTBO_PROP {
                match node.nop_property(prop) {
                    Err(FdtError::NotFound) => Ok(()), // allows not exists
                    other => other,
                }?;
            }
        }

        Ok(())
    }

    pub fn patch(&self, fdt: &mut Fdt) -> Result<()> {
        // Patches pre-populated PV IOMMUs
        let mut compatible = fdt.root_mut()?.next_compatible(Self::PVIOMMU_COMPATIBLE)?;
        let mut pviommu_phandles = BTreeMap::new();
        for pviommu in self.assigned_pviommus.values() {
            let mut assigned_pviommu = compatible.ok_or(DeviceAssignmentError::TooManyPvIommu)?;
            assigned_pviommu.setprop_inplace(cstr!("id"), &u32::to_be_bytes(pviommu.id.into()))?;
            let phandle = assigned_pviommu
                .as_node()
                .get_phandle()?
                .ok_or(DeviceAssignmentError::InvalidIommus)?;
            pviommu_phandles.insert(pviommu.id, phandle);
            compatible = assigned_pviommu.next_compatible(Self::PVIOMMU_COMPATIBLE)?;
        }

        // Filters pre-populated but unassigned PV IOMMUs.
        while let Some(filtered_pviommu) = compatible {
            compatible = filtered_pviommu.delete_and_next_compatible(Self::PVIOMMU_COMPATIBLE)?;
        }

        // Patches assigned devices
        for device in &self.assigned_devices {
            device.patch(fdt, &pviommu_phandles)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeSet;
    use std::fs;

    const VM_DTBO_FILE_PATH: &str = "test_pvmfw_devices_vm_dtbo.dtbo";
    const VM_DTBO_WITHOUT_SYMBOLS_FILE_PATH: &str =
        "test_pvmfw_devices_vm_dtbo_without_symbols.dtbo";
    const FDT_FILE_PATH: &str = "test_pvmfw_devices_with_rng.dtb";
    const FDT_WITH_IOMMU_FILE_PATH: &str = "test_pvmfw_devices_with_rng_iommu.dtb";
    const FDT_WITH_MULTIPLE_DEVICES_IOMMUS_FILE_PATH: &str =
        "test_pvmfw_devices_with_multiple_devices_iommus.dtb";
    const FDT_WITH_IOMMU_SHARING: &str = "test_pvmfw_devices_with_iommu_sharing.dtb";
    const FDT_WITH_IOMMU_ID_CONFLICT: &str = "test_pvmfw_devices_with_iommu_id_conflict.dtb";

    #[derive(Debug)]
    struct ExpectedNode {
        path: &'static CStr,
        reg: Vec<u8>,
        interrupts: Vec<u8>,
        iommu_ids: Vec<u32>,
    }

    fn assert_contains_node(fdt: &Fdt, expected_nodes: &[ExpectedNode]) {
        let mut all_iommu_ids: BTreeSet<u32> = BTreeSet::new();
        for expected in expected_nodes {
            let ExpectedNode { path, reg, interrupts, iommu_ids } = expected;
            let node = fdt.node(path).unwrap().unwrap();
            assert_eq!(
                node.getprop(cstr!("reg")),
                Ok(Some(reg.as_slice())),
                "Mismatch in reg of {path:?}"
            );
            assert_eq!(
                node.getprop(cstr!("interrupts")),
                Ok(Some(interrupts.as_slice())),
                "Mismatch in interrupts of {path:?}"
            );

            let cells = node.getprop_cells(cstr!("iommus")).unwrap().unwrap();
            let actual: Vec<_> = cells
                .map(|cell| {
                    let phandle = Phandle::try_from(cell).unwrap();
                    let pviommu = fdt.node_with_phandle(phandle).unwrap().unwrap();
                    let compatible = pviommu.getprop_str(cstr!("compatible"));
                    assert_eq!(compatible, Ok(Some(cstr!("pkvm,pviommu"))));
                    pviommu.getprop_u32(cstr!("id"))
                })
                .collect();

            let expected: Vec<_> = iommu_ids.iter().map(|id| Ok(Some(*id))).collect();
            assert_eq!(actual, expected);

            all_iommu_ids.extend(iommu_ids.iter());
        }

        assert_eq!(
            fdt.compatible_nodes(cstr!("pkvm,pviommu")).unwrap().count(),
            all_iommu_ids.len()
        );
    }

    fn into_fdt_prop(native_bytes: Vec<u32>) -> Vec<u8> {
        let mut v = Vec::with_capacity(native_bytes.len() * 4);
        for byte in native_bytes {
            v.extend_from_slice(&byte.to_be_bytes());
        }
        v
    }

    #[test]
    fn device_info_new_without_symbols() {
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let mut vm_dtbo_data = fs::read(VM_DTBO_WITHOUT_SYMBOLS_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();

        let device_info = DeviceAssignmentInfo::parse(fdt, vm_dtbo).unwrap();
        assert_eq!(device_info, None);
    }

    #[test]
    fn device_info_assigned_info() {
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();

        let device_info = DeviceAssignmentInfo::parse(fdt, vm_dtbo).unwrap().unwrap();

        let expected = [AssignedDeviceInfo {
            node_path: CString::new("/rng").unwrap(),
            dtbo_node_path: cstr!("/fragment@rng/__overlay__/rng").into(),
            reg: into_fdt_prop(vec![0x0, 0x9, 0x0, 0xFF]),
            interrupts: into_fdt_prop(vec![0x0, 0xF, 0x4]),
            iommu_ids: vec![],
        }];

        assert_eq!(device_info.assigned_devices, expected);
    }

    #[test]
    fn device_info_new_without_assigned_devices() {
        let mut fdt_data = vec![0; pvmfw_fdt_template::RAW.len()];
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let fdt = Fdt::create_empty_tree(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();

        let device_info = DeviceAssignmentInfo::parse(fdt, vm_dtbo).unwrap();
        assert_eq!(device_info, None);
    }

    #[test]
    fn device_info_filter() {
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();

        let device_info = DeviceAssignmentInfo::parse(fdt, vm_dtbo).unwrap().unwrap();
        device_info.filter(vm_dtbo).unwrap();

        let vm_dtbo = vm_dtbo.as_mut();

        let rng = vm_dtbo.node(cstr!("/fragment@rng/__overlay__/rng")).unwrap();
        assert_ne!(rng, None);

        let light = vm_dtbo.node(cstr!("/fragment@rng/__overlay__/light")).unwrap();
        assert_eq!(light, None);

        let symbols_node = vm_dtbo.symbols().unwrap();
        assert_eq!(symbols_node, None);
    }

    #[test]
    fn device_info_patch() {
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let mut data = vec![0_u8; fdt_data.len() + vm_dtbo_data.len()];
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let platform_dt = Fdt::create_empty_tree(data.as_mut_slice()).unwrap();

        let device_info = DeviceAssignmentInfo::parse(fdt, vm_dtbo).unwrap().unwrap();
        device_info.filter(vm_dtbo).unwrap();

        // SAFETY: Damaged VM DTBO wouldn't be used after this unsafe block.
        unsafe {
            platform_dt.apply_overlay(vm_dtbo.as_mut()).unwrap();
        }
        device_info.patch(platform_dt).unwrap();

        type FdtResult<T> = libfdt::Result<T>;
        let expected: Vec<(FdtResult<&CStr>, FdtResult<Vec<u8>>)> = vec![
            (Ok(cstr!("android,rng,ignore-gctrl-reset")), Ok(Vec::new())),
            (Ok(cstr!("compatible")), Ok(Vec::from(*b"android,rng\0"))),
            (Ok(cstr!("interrupts")), Ok(into_fdt_prop(vec![0x0, 0xF, 0x4]))),
            (Ok(cstr!("iommus")), Ok(Vec::new())),
            (Ok(cstr!("reg")), Ok(into_fdt_prop(vec![0x0, 0x9, 0x0, 0xFF]))),
        ];

        let rng_node = platform_dt.node(cstr!("/rng")).unwrap().unwrap();
        let mut properties: Vec<_> = rng_node
            .properties()
            .unwrap()
            .map(|prop| (prop.name(), prop.value().map(|x| x.into())))
            .collect();
        properties.sort_by(|a, b| {
            let lhs = a.0.unwrap_or_default();
            let rhs = b.0.unwrap_or_default();
            lhs.partial_cmp(rhs).unwrap()
        });

        assert_eq!(properties, expected);
    }

    #[test]
    fn device_info_overlay_iommu() {
        let mut fdt_data = fs::read(FDT_WITH_IOMMU_FILE_PATH).unwrap();
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let mut platform_dt_data = pvmfw_fdt_template::RAW.to_vec();
        platform_dt_data.resize(pvmfw_fdt_template::RAW.len() * 2, 0);
        let platform_dt = Fdt::from_mut_slice(&mut platform_dt_data).unwrap();
        platform_dt.unpack().unwrap();

        let device_info = DeviceAssignmentInfo::parse(fdt, vm_dtbo).unwrap().unwrap();
        device_info.filter(vm_dtbo).unwrap();

        // SAFETY: Damaged VM DTBO wouldn't be used after this unsafe block.
        unsafe {
            platform_dt.apply_overlay(vm_dtbo.as_mut()).unwrap();
        }
        device_info.patch(platform_dt).unwrap();

        let expected_devices = [ExpectedNode {
            path: cstr!("/rng"),
            reg: into_fdt_prop(vec![0x0, 0x9, 0x0, 0xFF]),
            interrupts: into_fdt_prop(vec![0x0, 0xF, 0x4]),
            iommu_ids: vec![0x4],
        }];

        assert_contains_node(platform_dt, &expected_devices);
    }

    #[test]
    fn device_info_multiple_devices_iommus() {
        let mut fdt_data = fs::read(FDT_WITH_MULTIPLE_DEVICES_IOMMUS_FILE_PATH).unwrap();
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let mut platform_dt_data = pvmfw_fdt_template::RAW.to_vec();
        platform_dt_data.resize(pvmfw_fdt_template::RAW.len() * 2, 0);
        let platform_dt = Fdt::from_mut_slice(&mut platform_dt_data).unwrap();
        platform_dt.unpack().unwrap();

        let device_info = DeviceAssignmentInfo::parse(fdt, vm_dtbo).unwrap().unwrap();
        device_info.filter(vm_dtbo).unwrap();

        // SAFETY: Damaged VM DTBO wouldn't be used after this unsafe block.
        unsafe {
            platform_dt.apply_overlay(vm_dtbo.as_mut()).unwrap();
        }
        device_info.patch(platform_dt).unwrap();

        let expected_devices = [
            ExpectedNode {
                path: cstr!("/rng"),
                reg: into_fdt_prop(vec![0x0, 0x9, 0x0, 0xFF]),
                interrupts: into_fdt_prop(vec![0x0, 0xF, 0x4]),
                iommu_ids: vec![0x4, 0x9],
            },
            ExpectedNode {
                path: cstr!("/light"),
                reg: into_fdt_prop(vec![0x100, 0x9]),
                interrupts: into_fdt_prop(vec![0x0, 0xF, 0x5]),
                iommu_ids: vec![0x40, 0x50, 0x60],
            },
        ];

        assert_contains_node(platform_dt, &expected_devices);
    }

    #[test]
    fn device_info_iommu_sharing() {
        let mut fdt_data = fs::read(FDT_WITH_IOMMU_SHARING).unwrap();
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let mut platform_dt_data = pvmfw_fdt_template::RAW.to_vec();
        platform_dt_data.resize(pvmfw_fdt_template::RAW.len() * 2, 0);
        let platform_dt = Fdt::from_mut_slice(&mut platform_dt_data).unwrap();
        platform_dt.unpack().unwrap();

        let device_info = DeviceAssignmentInfo::parse(fdt, vm_dtbo).unwrap().unwrap();
        device_info.filter(vm_dtbo).unwrap();

        // SAFETY: Damaged VM DTBO wouldn't be used after this unsafe block.
        unsafe {
            platform_dt.apply_overlay(vm_dtbo.as_mut()).unwrap();
        }
        device_info.patch(platform_dt).unwrap();

        let expected_devices = [
            ExpectedNode {
                path: cstr!("/rng"),
                reg: into_fdt_prop(vec![0x0, 0x9, 0x0, 0xFF]),
                interrupts: into_fdt_prop(vec![0x0, 0xF, 0x4]),
                iommu_ids: vec![0x4, 0x9],
            },
            ExpectedNode {
                path: cstr!("/light"),
                reg: into_fdt_prop(vec![0x100, 0x9]),
                interrupts: into_fdt_prop(vec![0x0, 0xF, 0x5]),
                iommu_ids: vec![0x9, 0x40],
            },
        ];

        assert_contains_node(platform_dt, &expected_devices);
    }

    #[test]
    fn device_info_iommu_id_conflict() {
        let mut fdt_data = fs::read(FDT_WITH_IOMMU_ID_CONFLICT).unwrap();
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();

        let device_info = DeviceAssignmentInfo::parse(fdt, vm_dtbo);

        assert_eq!(device_info, Err(DeviceAssignmentError::InvalidIommus));
    }
}
