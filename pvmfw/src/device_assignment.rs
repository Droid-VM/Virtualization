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

use alloc::ffi::CString;
use alloc::fmt;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::CStr;
use core::iter::Iterator;
use core::mem;
use libfdt::{Fdt, FdtError};

// TODO(b/308694211): Move this to the vmbase
macro_rules! const_cstr {
    ($str:literal) => {{
        #[allow(unused_unsafe)] // In case the macro is used within an unsafe block.
        // SAFETY: Trailing null is guaranteed by concat!()
        unsafe {
            CStr::from_bytes_with_nul_unchecked(concat!($str, "\0").as_bytes())
        }
    }};
}

const UNUSED_VM_DTBO_PROP: [&CStr; 3] = [
    const_cstr!("android,pvmfw,phy-reg"),
    const_cstr!("android,pvmfw,phy-iommu"),
    const_cstr!("android,pvmfw,phy-sid"),
];

const REG_PROP_NAME: &CStr = const_cstr!("reg");
const INTERRUPTS_PROP_NAME: &CStr = const_cstr!("interrupts");

// TODO(b/277993056): Keep constants derived from platform.dts in one place.
const CELLS_PER_INTERRUPT: usize = 3; // from /intc node in platform.dts

/// Errors in device assignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceAssignmentError {
    // Invalid VM DTBO
    InvalidDtbo,
    /// Invalid __symbols__
    InvalidSymbols,
    /// Invalid <reg>
    InvalidInterrupts,
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
            Self::UnsupportedOverlayTarget => {
                write!(f, "Unsupported overlay target. Only supports 'target-path = \"/\"'")
            }
            Self::UnexpectedFdtError(e) => write!(f, "Unexpected Error from libfdt: {e}"),
        }
    }
}

pub type Result<T> = core::result::Result<T, DeviceAssignmentError>;

/// Represents VM DTBO
/// Note: Does not implement AsRef or Deref because Fdt isn't Sized.
#[repr(transparent)]
pub struct VmDtbo(Fdt);

impl VmDtbo {
    const OVERLAY_NODE_NAME: &CStr = const_cstr!("__overlay__");
    const TARGET_PATH_PROP: &CStr = const_cstr!("target-path");

    /// Wraps a mutable slice containing a VM DTBO.
    ///
    /// Fails if the VM DTBO does not pass validation.
    pub fn from_mut_slice(dtbo: &mut [u8]) -> Result<&mut Self> {
        let _fdt = Fdt::from_mut_slice(dtbo)?;
        // Safety: Fdt::from_mut_slice() ensures safety of the transmute.
        Ok(unsafe { mem::transmute::<&mut [u8], &mut Self>(dtbo) })
    }

    /// Returns the underlying fdt as a reference.
    pub fn as_fdt(&self) -> &Fdt {
        &self.0
    }

    /// Returns the underlying fdt as a mutable reference.
    pub fn as_mut_fdt(&mut self) -> &mut Fdt {
        &mut self.0
    }

    // Returns overlay target path of a overlay node path. Overlay target path is the destination of
    // a overlay node after Fdt::apply_overlay() is applied.
    //
    // Contrary to fdt_overlay_target_offset(), this API enforces overlay target property
    // 'target-path = "/"', so the overlay doesn't modify and/or append platform DT's existing
    // node and/or properties.
    //
    // Here's an example with sample VM DTBO:
    //    / {
    //       fragment@eh {
    //         target-path = "/";  // Always 'target-path = "/"'. Disallows <target> or other path.
    //         __overlay__ {
    //           eh { ... };
    //         };
    //       };
    //       __symbols__ {  // List of assignable devices
    //         eh = "/fragment@eh/__overlay__/eh";   // Assignable device's path in VM DTBO
    //       };
    //    };
    //
    // Then vm_dtbo.overlay_target_path(cstr!("/fragment@eh/__overlay__/eh")) returns path "/eh"
    fn overlay_target_path(&self, overlay_node_path: &CStr) -> Result<CString> {
        let overlay_node_path_bytes = overlay_node_path.to_bytes();
        if overlay_node_path_bytes.first() != Some(&b'/') {
            return Err(DeviceAssignmentError::UnsupportedOverlayTarget);
        }

        let node = self.0.node(overlay_node_path)?.ok_or(DeviceAssignmentError::InvalidSymbols)?;

        let fragment_node = node.supernode_at_depth(1)?;
        let fragment_node_name_bytes = fragment_node.name()?.to_bytes();
        let target_path = fragment_node.getprop_str(Self::TARGET_PATH_PROP)?;
        if &target_path.unwrap_or_default().to_bytes() != b"/" {
            return Err(DeviceAssignmentError::UnsupportedOverlayTarget);
        }
        let overlay_node_name_bytes = Self::OVERLAY_NODE_NAME.to_bytes();
        let mut overlaid_path: Vec<u8> = Vec::with_capacity(
            overlay_node_path_bytes.len()          // e.g. /fragment@eh/__overlay__/eh
                - fragment_node_name_bytes.len()   // e.g.  fragment@eh
                - overlay_node_name_bytes.len()    // e.g.              __overlay__
                - 1, // remove double dash, but add one for null.
        );

        let mut depth = 0;
        for name in overlay_node_path_bytes.split(|char| char == &b'/') {
            if name.is_empty() {
                // This is expected for root and consecutive '/'.
                continue;
            }

            depth += 1;
            if depth == 2 && name != overlay_node_name_bytes {
                return Err(DeviceAssignmentError::InvalidDtbo);
            } else if depth > 2 {
                overlaid_path.push(b'/');
                overlaid_path.extend_from_slice(name);
            }
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

/// Assignable device information parsed from VM DTBO.
/// Assignable devices are listed in the /__symbols__ node in the VM DTBO.
/// Keeps everything in the owned data because VM DTBO will be damaged while overlaying.
#[derive(Debug)]
struct AssignableDeviceInfo {
    /// Assignable device node path in VM DTBO.
    /// This would be a value of a property in /__symbol__ node.
    assignable_dtbo_node_path: CString,
    // Overlay target path, which is path of the overlay node after VM DTBO is overlaid.
    // This is the same as the node path of assigned device in crosvm DT.
    overlay_target_path: CString,
}

/// Assigned device information parsed from crosvm DT.
/// Keeps everything in the owned data because underlying FDT will be reused for platform DT.
#[derive(Debug)]
struct AssignedDeviceInfo {
    // Node path of assigned device
    node_path: CString,
    // <reg> property from the crosvm DT
    reg: Vec<u8>,
    // <interrupts> property from the crosvm DT
    interrupts: Vec<u8>,
}

impl AssignedDeviceInfo {
    // TODO(b/277993056): Read and validate iommu
    fn new(fdt: &Fdt, node_path: &CStr) -> Result<Option<Self>> {
        let Some(node) = fdt.node(node_path)? else { return Ok(None) };

        // TODO(b/277993056): Validate reg with HVC, and keep reg with FdtNode::reg()
        let reg = node.getprop(REG_PROP_NAME).unwrap().unwrap();

        // interrupt must exist with cell numbers
        let interrupts: Vec<_> =
            node.getprop_cells(INTERRUPTS_PROP_NAME)?.ok_or(FdtError::Internal)?.collect();
        if interrupts.len() % CELLS_PER_INTERRUPT != 0 {
            return Err(DeviceAssignmentError::InvalidInterrupts);
        }

        // Once validated, keep the raw bytes so patch can be done with setprop()
        let interrupts = node.getprop(INTERRUPTS_PROP_NAME).unwrap().unwrap();

        Ok(Some(Self {
            node_path: node_path.into(),
            reg: reg.to_vec(),
            interrupts: interrupts.to_vec(),
        }))
    }
}

#[derive(Debug)]
pub struct DeviceAssignmentInfo {
    assignable_devices: Vec<AssignableDeviceInfo>,
    assigned_devices: Vec<AssignedDeviceInfo>,
}

impl DeviceAssignmentInfo {
    /// Creates new DeviceAssignmentInfo
    // TODO(b/277993056): Parse __local_fixups__
    // TODO(b/277993056): Parse __fixups__
    pub fn new(fdt: &Fdt, vm_dtbo: &VmDtbo) -> Result<Option<Self>> {
        let Some(symbols_node) = vm_dtbo.as_fdt().symbols()? else {
            // /__symbols__ should contain all assignable devices.
            // If empty, then nothing can be assigned.
            return Ok(None);
        };

        let mut assignable_devices = vec![];
        let mut assigned_devices = vec![];
        let mut assigned = false;
        for symbol_prop in symbols_node.properties()? {
            let symbol_prop_value = symbol_prop.value()?.to_vec();
            let assignable_dtbo_node_path = CString::from_vec_with_nul(symbol_prop_value)
                .or(Err(DeviceAssignmentError::InvalidSymbols))?;
            let overlay_target_path = vm_dtbo.overlay_target_path(&assignable_dtbo_node_path)?;
            assignable_devices
                .push(AssignableDeviceInfo { assignable_dtbo_node_path, overlay_target_path });
            let assignable_device = assignable_devices.last().unwrap();
            if let Some(assigned_device) =
                AssignedDeviceInfo::new(fdt, &assignable_device.overlay_target_path)?
            {
                assigned_devices.push(assigned_device);
                assigned = true;
            }
        }

        Ok(assigned.then_some(Self { assignable_devices, assigned_devices }))
    }

    /// Filters VM DTBO to only contain necessary information for booting pVM
    /// In detail, this will remove followings by setting nop node / nop property.
    ///   - Removes unassigned devices
    ///   - Removes /__symbols__ node
    // TODO(b/277993056): remove unused dependencies in VM DTBO.
    // TODO(b/277993056): remove supernodes' properties.
    // TODO(b/277993056): remove unused alises.
    pub fn filter(&self, vm_dtbo: &mut VmDtbo) -> Result<()> {
        let vm_dtbo = vm_dtbo.as_mut_fdt();

        // Filters unused node/properties.
        for assignable_device in &self.assignable_devices {
            let device = self
                .assigned_devices
                .iter()
                .find(|device| assignable_device.overlay_target_path == device.node_path);
            if device.is_some() {
                let mut node = vm_dtbo
                    .node_mut(&assignable_device.assignable_dtbo_node_path)
                    .unwrap()
                    .unwrap();
                for prop in UNUSED_VM_DTBO_PROP {
                    node.nop_property(prop)?;
                }
            } else {
                let node = vm_dtbo
                    .node_mut(&assignable_device.assignable_dtbo_node_path)
                    .unwrap()
                    .unwrap();
                node.nop()?;
            }
        }

        // Removes __symbols__
        let symbols_node = vm_dtbo.symbols_mut().unwrap().unwrap();
        Ok(symbols_node.nop()?)
    }

    pub fn patch(&self, fdt: &mut Fdt) -> Result<()> {
        // Patch reg, interrupts, iommus.
        // TODO(b/277993056): Handle __aliases__
        for device in &self.assigned_devices {
            let mut dst = fdt.node_mut(&device.node_path)?.unwrap();
            dst.setprop(REG_PROP_NAME, &device.reg)?;
            dst.setprop(INTERRUPTS_PROP_NAME, &device.interrupts)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // TODO(b/308694211): Use cstr! from vmbase instead.
    macro_rules! cstr {
        ($str:literal) => {{
            CStr::from_bytes_with_nul(concat!($str, "\0").as_bytes()).unwrap()
        }};
    }

    const VM_DTBO_FILE_PATH: &str = "test_pvmfw_devices_vm_dtbo.dtbo";
    const VM_DTBO_WITHOUT_SYMBOLS_FILE_PATH: &str =
        "test_pvmfw_devices_vm_dtbo_without_symbols.dtbo";
    const FDT_FILE_PATH: &str = "test_pvmfw_devices_with_eh.dtb";

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

        let device_info = DeviceAssignmentInfo::new(fdt, vm_dtbo).unwrap();
        assert!(device_info.is_none(), "Expected None but was {device_info:?}");
    }

    #[test]
    fn device_info_assigned_info() {
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();

        let device_info = DeviceAssignmentInfo::new(fdt, vm_dtbo).unwrap().unwrap();

        let expected_path = cstr!("/eh");
        let expected_reg = into_fdt_prop(vec![0x0, 0x9, 0x0, 0xFF]);
        let expected_interrupts = into_fdt_prop(vec![0x0, 0xF, 0x4]);

        assert_eq!(device_info.assigned_devices.len(), 1);
        let AssignedDeviceInfo { node_path, reg, interrupts } = &device_info.assigned_devices[0];
        assert_eq!(node_path.as_c_str(), expected_path);
        assert_eq!(reg, &expected_reg);
        assert_eq!(interrupts, &expected_interrupts);
    }

    #[test]
    fn device_info_new_without_assigned_devices() {
        let mut fdt_data: Vec<u8> = pvmfw_fdt_template::RAW.into();
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(fdt_data.as_mut_slice()).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();

        let device_info = DeviceAssignmentInfo::new(fdt, vm_dtbo).unwrap();
        assert!(device_info.is_none(), "Expected None but was {device_info:?}");
    }

    #[test]
    fn device_info_filter() {
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();

        let device_info = DeviceAssignmentInfo::new(fdt, vm_dtbo).unwrap().unwrap();
        device_info.filter(vm_dtbo).unwrap();

        let vm_dtbo = vm_dtbo.as_fdt();

        let eh = vm_dtbo.node(cstr!("/fragment@eh/__overlay__/eh")).unwrap();
        assert!(eh.is_some());

        let light = vm_dtbo.node(cstr!("/fragment@eh/__overlay__/light")).unwrap();
        assert!(light.is_none());

        let symbols_node = vm_dtbo.symbols().unwrap();
        assert!(symbols_node.is_none());
    }

    #[test]
    fn device_info_patch() {
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let mut data = vec![0_u8; fdt_data.len() + vm_dtbo_data.len()];
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let vm_dtbo = VmDtbo::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let platform_dt = Fdt::create_empty_tree(data.as_mut_slice()).unwrap();

        let device_info = DeviceAssignmentInfo::new(fdt, vm_dtbo).unwrap().unwrap();
        device_info.filter(vm_dtbo).unwrap();

        // SAFETY: unsafe call in test code
        unsafe {
            platform_dt.apply_overlay(vm_dtbo.as_mut_fdt()).unwrap();
        }

        let eh_node = platform_dt.node(cstr!("/eh")).unwrap().unwrap();
        let expected: Vec<(&CStr, Vec<u8>)> = vec![
            (cstr!("android,eh,ignore-gctrl-reset"), Vec::<u8>::new()),
            (cstr!("compatible"), b"android,eh\0".to_vec()),
            (cstr!("reg"), into_fdt_prop(vec![0x0, 0x9, 0x0, 0xFF])),
            (cstr!("interrupts"), into_fdt_prop(vec![0x0, 0xF, 0x4])),
        ];

        for (prop, (prop_name, prop_value)) in eh_node.properties().unwrap().zip(expected) {
            assert_eq!(prop.name(), Ok(prop_name));
            assert_eq!(prop.value(), Ok(prop_value.as_slice()));
        }
    }
}
