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
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::CStr;
use core::iter::Iterator;
use libfdt::{Fdt, FdtError};

macro_rules! const_cstr {
    ($str:literal) => {{
        #[allow(unused_unsafe)] // In case the macro is used within an unsafe block.
        // SAFETY: Trailing is null is gauaranteed by concat!()
        unsafe {
            CStr::from_bytes_with_nul_unchecked(concat!($str, "\0").as_bytes())
        }
    }};
}

const SYMBOLS_NODE_PATH: &CStr = const_cstr!("/__symbols__");
const OVERLAY_NODE_NAME: &CStr = const_cstr!("__overlay__");
const TARGET_PATH_PROP: &CStr = const_cstr!("target-path");
const UNUSED_VM_DTBO_PROP: [&CStr; 3] = [
    const_cstr!("android,pvmfw,phy-reg"),
    const_cstr!("android,pvmfw,phy-iommu"),
    const_cstr!("android,pvmfw,phy-sid"),
];

const REG_PROP_NAME: &CStr = const_cstr!("reg");
const INTERRUPTS_PROP_NAME: &CStr = const_cstr!("interrupts");

const INTERRUPTS_CELL_NUM: usize = 3; // from /intc node in platform.dts

/// Assignable device information parsed from VM DTBO.
///
/// This keeps raw ptrs to avoid borrow checker issue for having immutable borrow (symbol_*)
/// when mutable borrow (vm_dtbo) exists.
#[derive(Debug)]
struct AssignableDevice {
    path: CString,
    symbol_name_ptr: *const u8,
    symbol_path_ptr: *const u8,
}

/// Assigned device information parsed from crosvm DT.
/// Keeps everything in the owned data because underlying FDT will be reused for platform DT.
#[derive(Debug)]
struct AssignedDevice {
    reg: Vec<u8>,
    interrupts: Vec<u8>,
}

impl AssignedDevice {
    // TODO(b/277993056): Read and validate iommu
    fn new_validated(fdt: &Fdt, node_path: &CStr) -> Result<Option<Self>, FdtError> {
        let Some(node) = fdt.node(node_path)? else { return Ok(None) };

        // reg must exist and format must be valid
        // TODO(b/277993056): Validate reg with HVC
        let _reg: Vec<_> = node.reg()?.ok_or(FdtError::Internal)?.collect();

        // Once validated, keep the raw bytes so patch can be done with setprop()
        let reg = node.getprop(REG_PROP_NAME)?.unwrap();

        // interrupt must exist with valid format
        let interrupts: Vec<_> =
            node.getprop_cells(INTERRUPTS_PROP_NAME)?.ok_or(FdtError::Internal)?.collect();
        if interrupts.len() != INTERRUPTS_CELL_NUM {
            return Err(FdtError::Internal);
        }
        // Once validated, keep the raw bytes so patch can be done with setprop()
        let interrupts = node.getprop(INTERRUPTS_PROP_NAME)?.unwrap();

        Ok(Some(Self { reg: reg.to_vec(), interrupts: interrupts.to_vec() }))
    }
}

#[derive(Debug)]
pub struct DeviceInfo<'a> {
    vm_dtbo: &'a mut Fdt,
    devices: Vec<(AssignableDevice, Option<AssignedDevice>)>,
}

impl<'a> DeviceInfo<'a> {
    // Returns overlaid path with validations.
    fn to_overlaid_path(fdt: &Fdt, overlay_node_path: &CStr) -> Result<CString, FdtError> {
        let overlay_node_path_bytes = overlay_node_path.to_bytes();
        if overlay_node_path_bytes.first() != Some(&b'/') {
            // We wouldn't allow overlay onto symbol/aliases in the platform DT.
            return Err(FdtError::BadOverlay);
        }

        let node = fdt.node(overlay_node_path)?.ok_or(FdtError::BadOverlay)?;

        let fragment_node = node.supernode_at_depth(1)?;
        let fragment_node_name_bytes = fragment_node.name()?.to_bytes();
        let target_path =
            fragment_node.getprop_str(TARGET_PATH_PROP)?.ok_or(FdtError::BadOverlay)?.to_bytes();
        let trimed_target_path_bytes = target_path.strip_suffix(b"/").unwrap_or(target_path);
        let overlay_node_name_bytes = OVERLAY_NODE_NAME.to_bytes();
        let mut overlaid_path: Vec<u8> = Vec::with_capacity(
            trimed_target_path_bytes.len() + overlay_node_path_bytes.len()
                - fragment_node_name_bytes.len()
                - overlay_node_name_bytes.len()
                - 1,
        );
        overlaid_path.extend_from_slice(trimed_target_path_bytes);

        let mut depth = 0;
        for name in overlay_node_path_bytes.split(|char| char == &b'/') {
            if name.is_empty() {
                // This is expected for root and consecutive '/'.
                continue;
            }

            depth += 1;
            if depth == 2 && name != overlay_node_name_bytes {
                return Err(FdtError::BadOverlay);
            } else if depth > 2 {
                overlaid_path.push(b'/');
                overlaid_path.extend_from_slice(name);
            }
        }
        overlaid_path.push(b'\0');

        Ok(CString::from_vec_with_nul(overlaid_path).unwrap())
    }

    /// Creates new DeviceInfo with validation.
    // TODO(b/277993056): Parse __local_fixups__
    // TODO(b/277993056): Parse __fixups__
    pub fn new_validated(vm_dtbo: &'a mut Fdt, fdt: &Fdt) -> Result<Option<Self>, FdtError> {
        let mut devices = vec![];
        let Some(symbols_node) = vm_dtbo.node(SYMBOLS_NODE_PATH)? else {
            // __symbols__ should contain all assignable devices.
            // If empty, then nothing can be assigned.
            return Ok(None);
        };

        let mut has_assigned_device = false;
        for symbol in symbols_node.properties()? {
            // Parse VM DTBO
            let symbol_name = symbol.name()?;
            let symbol_path =
                CStr::from_bytes_with_nul(symbol.value()?).or(Err(FdtError::Internal))?;
            let path = Self::to_overlaid_path(vm_dtbo, symbol_path)?;

            let assigned_device = AssignedDevice::new_validated(fdt, &path)?;
            has_assigned_device |= assigned_device.is_some();

            // Convert symbol_name and symbol_path to ptr to keep it together with vm_dtbo.
            // Otherwise, borrow checker will complaint about immutable borrow (symbol_*)
            // when mutable borrow (vm_dtbo) exists.
            devices.push((
                AssignableDevice {
                    path,
                    symbol_name_ptr: symbol_name.as_ptr(),
                    symbol_path_ptr: symbol_path.as_ptr(),
                },
                assigned_device,
            ));
        }

        Ok(has_assigned_device.then_some(Self { vm_dtbo, devices }))
    }

    /// Applies VM DTBO overlay onto fdt, but only with assigned devices. And patches device info.
    /// This consumes self because containing VM DTBO is damaged to trim unassigned devices and
    /// overlay.
    pub fn apply_overlay_and_patch(self, fdt: &mut Fdt) -> Result<(), FdtError> {
        let DeviceInfo { vm_dtbo, devices } = self;

        // Clean up unused node and unused properties.
        for (assignable_device, assigned_device) in &devices {
            // SAFETY: This function only modifies underlying vm_dtbo with nop_property() and nop(),
            // which only alters bytes in the blobs for tag and doesn't alter nor move any other
            // part of the tree. ptr obtained by new() remains valid here.
            let symbol_path = unsafe { CStr::from_ptr(assignable_device.symbol_path_ptr) };
            if assigned_device.is_some() {
                let mut overlay_node = vm_dtbo.node_mut(symbol_path).unwrap().unwrap();
                for prop in UNUSED_VM_DTBO_PROP {
                    overlay_node.nop_property(prop)?;
                }
            } else {
                // Remove unused VM DTBO node
                // Empty fragment will be ingored, so We don't need to care overlay fragment.
                let node = vm_dtbo.node_mut(symbol_path)?.unwrap();
                node.nop()?;

                // TODO(b/277993056): remove supernodes' properties.
                // TODO(b/277993056): remove unused alises.
            }
        }

        // Clean up unused symbols in __symbols__
        // new_from_vm_dtbo() guarantees the existence of __symbols__.
        let mut symbols_node = vm_dtbo.node_mut(SYMBOLS_NODE_PATH).unwrap().unwrap();
        for (assignable_device, assigned_device) in &devices {
            if assigned_device.is_some() {
                continue;
            }
            // SAFETY: This function only modifies underlying vm_dtbo with nop_property() and nop(),
            // which only alters bytes in the blobs for tag and doesn't alter nor move any other
            // part of the tree. ptr obtained by new() remains valid here.
            let symbol_name = unsafe { CStr::from_ptr(assignable_device.symbol_name_ptr) };
            symbols_node.nop_property(symbol_name)?;
        }

        // SAFETY: vm_dtbo isn't used afterward.
        unsafe {
            fdt.apply_overlay(vm_dtbo)?;
        }

        // Patch reg, interrupts, iommus.
        // TODO(b/277993056): Handle __aliases__
        for (assignable_device, assigned_device) in &devices {
            let Some(device) = assigned_device else {
                continue;
            };
            let mut dst = fdt.node_mut(&assignable_device.path)?.unwrap();

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

    const VM_DTBO_FILE_PATH: &str = "test_pvmfw_devices_vm_dtbo.dtbo";
    const VM_DTBO_WITHOUT_SYMBOLS_FILE_PATH: &str =
        "test_pvmfw_devices_vm_dtbo_without_symbols.dtbo";
    const VM_DTBO_OVERLAY_NONROOT_FILE_PATH: &str =
        "test_pvmfw_devices_vm_dtbo_overlay_nonroot.dtbo";
    const FDT_FILE_PATH: &str = "test_pvmfw_devices_with_eh.dtb";
    const FDT_WITHOUT_ASSIGNED_DEVICE_FILE_PATH: &str =
        "test_pvmfw_devices_without_assigned_devices.dtb";

    fn into_fdt_prop(native_bytes: Vec<u32>) -> Vec<u8> {
        let mut v = Vec::with_capacity(native_bytes.len() * 4);
        for byte in native_bytes {
            v.extend_from_slice(&byte.to_be_bytes());
        }
        v
    }

    #[test]
    fn device_info_contains_all_symbols() {
        let mut vm_dtbo_data = fs::read(VM_DTBO_OVERLAY_NONROOT_FILE_PATH).unwrap();
        let vm_dtbo = Fdt::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();

        let device_info = DeviceInfo::new_validated(vm_dtbo, fdt).unwrap().unwrap();

        let expected = vec![
            ("/fragment@eh/__overlay__/eh", "/eh"),
            ("/fragment@sensor/__overlay__/sensor/light", "/pci/sensor/light"),
        ];

        for ((device, _), (symbol_path, path)) in device_info.devices.iter().zip(expected) {
            // SAFETY: Unsafe block for test.
            let device_symbol_path = unsafe { CStr::from_ptr(device.symbol_path_ptr) };
            assert_eq!(device_symbol_path.to_str().unwrap(), symbol_path);
            assert_eq!(device.path.to_str().unwrap(), path);
        }
    }

    #[test]
    fn device_info_new_without_symbols() {
        let mut vm_dtbo_data = fs::read(VM_DTBO_WITHOUT_SYMBOLS_FILE_PATH).unwrap();
        let vm_dtbo = Fdt::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();

        let device_info = DeviceInfo::new_validated(vm_dtbo, fdt).unwrap();
        assert!(device_info.is_none());
    }

    #[test]
    fn device_info_assigned_info() {
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let vm_dtbo = Fdt::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();

        let device_info = DeviceInfo::new_validated(vm_dtbo, fdt).unwrap().unwrap();

        let expected_path = const_cstr!("/eh");
        let expected_reg = into_fdt_prop(vec![0x0, 0x9, 0x0, 0xFF]);
        let expected_interrupts = into_fdt_prop(vec![0x0, 0xF, 0x4]);

        for (assignable_device, assigned_device) in &device_info.devices {
            if assignable_device.path.as_c_str() == expected_path {
                let assigned_device = assigned_device.as_ref().unwrap();
                assert_eq!(assigned_device.reg, expected_reg);
                assert_eq!(assigned_device.interrupts, expected_interrupts);
            } else {
                assert!(assigned_device.is_none());
            }
        }
    }

    #[test]
    fn device_info_new_without_assigned_devices() {
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let vm_dtbo = Fdt::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let mut fdt_data = fs::read(FDT_WITHOUT_ASSIGNED_DEVICE_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();

        let device_info = DeviceInfo::new_validated(vm_dtbo, fdt).unwrap();
        assert!(device_info.is_none());
    }

    #[test]
    fn device_info_apply_overlay_and_patch() {
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let vm_dtbo = Fdt::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();
        let mut data = vec![0_u8; fdt.as_slice().len() + vm_dtbo.as_slice().len()];
        let platform_dt = Fdt::create_empty_tree(data.as_mut_slice()).unwrap();

        let device_info = DeviceInfo::new_validated(vm_dtbo, fdt).unwrap().unwrap();
        device_info.apply_overlay_and_patch(platform_dt).unwrap();

        let eh_node = platform_dt.node(const_cstr!("/eh")).unwrap().unwrap();
        let expected: Vec<(&str, Vec<u8>)> = vec![
            ("interrupts", into_fdt_prop(vec![0x0, 0xF, 0x4])),
            ("reg", into_fdt_prop(vec![0x0, 0x9, 0x0, 0xFF])),
            ("android,eh,ignore-gctrl-reset", Vec::<u8>::new()),
            ("compatible", b"android,eh\0".to_vec()),
        ];

        for (prop, (prop_name, prop_value)) in eh_node.properties().unwrap().zip(expected) {
            assert_eq!(prop.name().unwrap().to_str().unwrap(), prop_name);
            assert_eq!(prop.value().unwrap(), prop_value.as_slice());
        }
    }
}
