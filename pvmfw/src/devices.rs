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
use libfdt::{Fdt, FdtError};

macro_rules! const_cstr {
    ($str:literal) => {{
        #[allow(unused_unsafe)] // In case the macro is used within an unsafe block.
        // SAFETY: Trailing null is guaranteed by concat!()
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
const INTERRUPTS_CELLS_NUM: usize = 3; // from /intc node in platform.dts

/// Error type corresponding to libfdt error codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceError {
    // Invalid VM DTBO
    InvalidDtbo,
    /// Invalid __symbols__
    InvalidSymbols,
    /// Invalid <reg>
    InvalidReg,
    /// Invalid <reg>
    InvalidInterrupts,
    /// Failure when overlay VM DTBO
    FailedOverlay(FdtError),
    /// Unsupported overlay target syntax. Only supports <target-path> with full path.
    UnsupportedOverlayTarget,
    /// Unexpected error from libfdt
    UnexpectedFdtError(FdtError),
}

impl From<FdtError> for DeviceError {
    fn from(e: FdtError) -> Self {
        DeviceError::UnexpectedFdtError(e)
    }
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidDtbo => write!(f, "Invalid DTBO"),
            Self::InvalidSymbols => write!(
                f,
                "Invalid property in /__symbols__. Must point to valid assignable device node."
            ),
            Self::InvalidReg => write!(f, "Invalid <reg>"),
            Self::InvalidInterrupts => write!(f, "Invalid <interrupts>"),
            Self::FailedOverlay(e) => write!(f, "Failed to apply VM DTBO: {e}"),
            Self::UnsupportedOverlayTarget => {
                write!(f, "Unsupported overlay target. Only supports 'target-path = \"/\"'")
            }
            Self::UnexpectedFdtError(e) => write!(f, "Unexpected Error from libfdt: {e}"),
        }
    }
}

/// Assignable device information parsed from VM DTBO. Assignable devices are listed in the
/// /__symbols__ node in the VM DTBO.
///
/// This keeps raw ptr of the symbol path (i.e. property value in /__symbols__) to bypass borrow
/// checker for having immutable borrow (symbol_path_ptr) when mutable borrow (vm_dtbo) exists.
#[derive(Debug)]
struct AssignableDevice {
    /// Path after the assignable device is overlaid (a.k.a. overlaid path).
    path: CString,
    /// Path from the __symbols__ node in the VM DTBO which describes assignable device's path.
    symbol_path_ptr: *const u8,
}

impl AssignableDevice {
    // Returns overlaid path of a overlay node.
    //
    // Here's an example with sample VM DTBO:
    //    / {
    //       fragment@eh {
    //         target-path = "/";  // Currently can only be overlaid by 'target-path = "/"'
    //         __overlay__ {
    //           eh { ... };
    //         };
    //       };
    //       __symbols__ {  // List of assignable devices
    //         eh = "/fragment@eh/__overlay__/eh";   // Assignable device's path in VM DTBO
    //       };
    //    };
    //
    // - overlay node path is assignable device node in the VM DTBO's /__symbols__. It would be
    //   overlaid to platform DT. (e.g. "/fragment@eh/__overlay__/eh")
    // - overlaid path is the destination of the overlay node path after being overlaid. (e.g.
    //   "/eh")
    fn to_overlaid_path(fdt: &Fdt, overlay_node_path: &CStr) -> Result<CString, DeviceError> {
        let overlay_node_path_bytes = overlay_node_path.to_bytes();
        if overlay_node_path_bytes.first() != Some(&b'/') {
            return Err(DeviceError::UnsupportedOverlayTarget);
        }

        let node = fdt.node(overlay_node_path)?.ok_or(DeviceError::InvalidSymbols)?;

        let fragment_node = node.supernode_at_depth(1)?;
        let fragment_node_name_bytes = fragment_node.name()?.to_bytes();
        let target_path = fragment_node.getprop_str(TARGET_PATH_PROP)?;
        if &target_path.unwrap_or_default().to_bytes() != b"/" {
            return Err(DeviceError::UnsupportedOverlayTarget);
        }
        let overlay_node_name_bytes = OVERLAY_NODE_NAME.to_bytes();
        let mut overlaid_path: Vec<u8> = Vec::with_capacity(
            overlay_node_path_bytes.len()          // /fragment@eh/__overlay__/eh
                - fragment_node_name_bytes.len()   //  fragment@eh
                - overlay_node_name_bytes.len()    //              __overlay__
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
                return Err(DeviceError::InvalidDtbo);
            } else if depth > 2 {
                overlaid_path.push(b'/');
                overlaid_path.extend_from_slice(name);
            }
        }
        overlaid_path.push(b'\0');

        Ok(CString::from_vec_with_nul(overlaid_path).unwrap())
    }

    /// Returns a new AssignableDevice from a symbol path written in the /__symbols__ node.
    ///
    /// # Safety
    ///
    /// Returned AssignableDevice has a ptr that points a property of /__systems__ node inside of
    /// the VM DTBO. VM DTBO's /__symbols__ node must be unmodified while AssignableDevice exists.
    // TODO(b/277993056): Read and validate iommu
    unsafe fn new_with_symbol_prop(vm_dtbo: &Fdt, symbol_prop: &[u8]) -> Result<Self, DeviceError> {
        let symbol_path =
            CStr::from_bytes_with_nul(symbol_prop).or(Err(DeviceError::InvalidSymbols))?;
        let path = Self::to_overlaid_path(vm_dtbo, symbol_path)?;
        Ok(Self { path, symbol_path_ptr: symbol_path.as_ptr() })
    }
}

/// Assigned device information parsed from crosvm DT.
/// Keeps everything in the owned data because underlying FDT will be reused for platform DT.
#[derive(Debug)]
struct AssignedDevice {
    path: CString,
    reg: Vec<u8>,
    interrupts: Vec<u8>,
}

impl AssignedDevice {
    // TODO(b/277993056): Read and validate iommu
    fn from_node_path(fdt: &Fdt, path: CString) -> Result<Option<Self>, DeviceError> {
        let Some(node) = fdt.node(&path)? else { return Ok(None) };

        // reg must exist and format must be valid
        // TODO(b/277993056): Validate reg with HVC
        let _reg: Vec<_> = node.reg()?.ok_or(DeviceError::InvalidReg)?.collect();

        // Once validated, keep the raw bytes so patch can be done with setprop()
        let reg = node.getprop(REG_PROP_NAME).unwrap().unwrap();

        // interrupt must exist with valid format
        let interrupts: Vec<_> =
            node.getprop_cells(INTERRUPTS_PROP_NAME)?.ok_or(FdtError::Internal)?.collect();
        if interrupts.len() % INTERRUPTS_CELLS_NUM != 0 {
            return Err(DeviceError::InvalidInterrupts);
        }
        // Once validated, keep the raw bytes so patch can be done with setprop()
        let interrupts = node.getprop(INTERRUPTS_PROP_NAME).unwrap().unwrap();

        Ok(Some(Self { path, reg: reg.to_vec(), interrupts: interrupts.to_vec() }))
    }
}

#[derive(Debug)]
pub struct DeviceInfo<'a> {
    vm_dtbo: &'a mut Fdt,
    devices: Vec<AssignedDevice>,
}

impl<'a> DeviceInfo<'a> {
    /// Creates new DeviceInfo with filtered by VM DTBO.
    // TODO(b/277993056): Parse __local_fixups__
    // TODO(b/277993056): Parse __fixups__
    pub fn new_filtered(vm_dtbo: &'a mut Fdt, fdt: &Fdt) -> Result<Option<Self>, DeviceError> {
        let Some(symbols_node) = vm_dtbo.node(SYMBOLS_NODE_PATH)? else {
            // __symbols__ should contain all assignable devices.
            // If empty, then nothing can be assigned.
            return Ok(None);
        };

        let mut assignable_devices = vec![];
        for symbol_prop in symbols_node.properties()? {
            // SAFETY: assignable devices would be only used while /__symbols__ is unmodified.
            // And reference to the vm_dtbo (symbol_path) would be dropped in next for loop.
            let device = unsafe {
                AssignableDevice::new_with_symbol_prop(
                    vm_dtbo,
                    symbol_prop.value().or(Err(DeviceError::InvalidSymbols))?,
                )?
            };
            assignable_devices.push(device);
        }

        let mut devices = vec![];
        let mut has_assigned_device = false;
        for assignable_device in assignable_devices {
            let AssignableDevice { path, symbol_path_ptr } = assignable_device;
            // SAFETY: This loop doesn't modify /__symbols__ node. symbol_path_ptr remains valid.
            let symbol_path = unsafe { CStr::from_ptr(symbol_path_ptr) };
            let assigned_device = AssignedDevice::from_node_path(fdt, path)?;
            if let Some(device) = assigned_device {
                has_assigned_device = true;

                let mut node = vm_dtbo.node_mut(symbol_path).unwrap().unwrap();
                for prop in UNUSED_VM_DTBO_PROP {
                    node.nop_property(prop)?;
                }
                devices.push(device);
            } else {
                let node = vm_dtbo.node_mut(symbol_path).unwrap().unwrap();
                node.nop()?;

                // TODO(b/277993056): remove supernodes' properties.
                // TODO(b/277993056): remove unused alises.
            }
        }

        // Removes /__symbols__ because it should be only used to specify list of assignable
        // devices.
        let symbols_node = vm_dtbo.node_mut(SYMBOLS_NODE_PATH).unwrap().unwrap();
        symbols_node.nop()?;

        Ok(has_assigned_device.then_some(Self { vm_dtbo, devices }))
    }

    /// Applies VM DTBO overlay onto fdt, but only with assigned devices. And patches device info.
    ///
    /// # Safety
    ///
    /// This damages containing VM DTBO for trimming unassigned devices and overlaying. VM DTBO
    /// shouldn't be used afterward.
    pub unsafe fn apply_overlay_and_patch(self, fdt: &mut Fdt) -> Result<(), DeviceError> {
        let DeviceInfo { vm_dtbo, devices } = self;

        // TODO(b/277993056): ensure that apply_overlay() wouldn't modify platform DT node.
        // SAFETY: vm_dtbo isn't used afterward.
        unsafe {
            fdt.apply_overlay(vm_dtbo).map_err(DeviceError::FailedOverlay)?;
        }

        // Patch reg, interrupts, iommus.
        // TODO(b/277993056): Handle __aliases__
        for device in &devices {
            let mut dst = fdt.node_mut(&device.path)?.unwrap();

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
    fn device_info_new_without_symbols() {
        let mut vm_dtbo_data = fs::read(VM_DTBO_WITHOUT_SYMBOLS_FILE_PATH).unwrap();
        let vm_dtbo = Fdt::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();

        let device_info = DeviceInfo::new_filtered(vm_dtbo, fdt).unwrap();
        assert!(device_info.is_none());
    }

    #[test]
    fn device_info_assigned_info() {
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let vm_dtbo = Fdt::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let mut fdt_data = fs::read(FDT_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();

        let device_info = DeviceInfo::new_filtered(vm_dtbo, fdt).unwrap().unwrap();

        let expected_path = const_cstr!("/eh");
        let expected_reg = into_fdt_prop(vec![0x0, 0x9, 0x0, 0xFF]);
        let expected_interrupts = into_fdt_prop(vec![0x0, 0xF, 0x4]);

        assert_eq!(device_info.devices.len(), 1);
        let AssignedDevice { path, reg, interrupts } = &device_info.devices[0];
        assert_eq!(path.as_c_str(), expected_path);
        assert_eq!(reg, &expected_reg);
        assert_eq!(interrupts, &expected_interrupts);
    }

    #[test]
    fn device_info_new_without_assigned_devices() {
        let mut vm_dtbo_data = fs::read(VM_DTBO_FILE_PATH).unwrap();
        let vm_dtbo = Fdt::from_mut_slice(&mut vm_dtbo_data).unwrap();
        let mut fdt_data = fs::read(FDT_WITHOUT_ASSIGNED_DEVICE_FILE_PATH).unwrap();
        let fdt = Fdt::from_mut_slice(&mut fdt_data).unwrap();

        let device_info = DeviceInfo::new_filtered(vm_dtbo, fdt).unwrap();
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

        let device_info = DeviceInfo::new_filtered(vm_dtbo, fdt).unwrap().unwrap();
        // SAFETY: unsafe call in test code
        unsafe {
            device_info.apply_overlay_and_patch(platform_dt).unwrap();
        }

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
