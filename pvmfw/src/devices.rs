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

//! Validates devices in device tree.
//! Declared in separated libs for adding unit tests, which requires libstd.

use crate::reboot_reason::RebootReason;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::ffi::CStr;
use core::rc::Rc;
use libfdt::{Fdt, FdtError};

macro_rules! const_cstr {
    ($str:literal) => {{
        #[allow(unused_unsafe)] // In case the macro is used within an unsafe block.
        unsafe {
            CStr::from_bytes_with_nul_unchecked(concat!($str, "\0").as_bytes())
        }
    }};
}

const SYMBOLS_NODE_PATH: CStr = const_cstr!("/__symbols__");
const OVERLAY_NODE_NAME: CStr = const_cstr!("__overlay__");
const LOCAL_FIXUPS_NODE_PATH: CStr = const_cstr!("/__local_fixups__");
const TARGET_PATH_PROP: CStr = const_cstr!("target-path");
const PHANDLE_PROP_NAME: CStr = const_cstr!("phandle");
const UNUSED_VM_DTBO_PROP: [CStr; 3] = [
    const_cstr!("android,pvmfw,phy-reg"),
    const_cstr!("android,pvmfw,phy-iommu"),
    const_cstr!("android,pvmfw,phy-sid"),
];

const REG_PROP_NAME: CStr = const_cstr!("reg");
const INTERRUPT_PROP_NAME: CStr = const_cstr!("bbbbb???");

#[derive(Debug, Copy)]
struct AssignableDeviceInfo {
    overlaid_path: Rc<String>,
    symbol_name: &CStr,
    symbol_path: &CStr,
}

/// VM DTBO information which contains parsed assignable devices and FDT.
#[derive(Debug, Copy)]
struct VmDtboInfo {
    vm_dtbo: &mut Fdt,
    assignable_devices: Vec<AssignableDeviceInfo>,
}

impl VmDtboInfo {
    // Finds overlaid path with validations.
    fn find_overlaid_path(fdt: &mut Fdt, node_path: &CStr) -> Result<CString, FdtError> {
        let node = fdt.node(node_path)?.ok_or(FdtError::BadOverlay)?;

        let fragment_node = node.supernode_at_depth(1)?;
        let target_path = fragment_node.getprop_str(TARGET_PATH_PROP)?;

        // Validate __overlay__ exists next to the fragment node
        let fragment_node_name_len = fragment_node.name()?.to_bytes().len();
        let path = node_path.to_bytes()[fragment_node_name_len + 1..];
        if !path.starts_with(OVERLAY_NODE_NAME.to_bytes()) {
            return Err(FdtError::BadOverlay);
        }

        let relative_path_bytes = node_path.to_bytes_with_nul()
            [fragment_node_name_len + OVERLAY_NODE_NAME.to_bytes().len() + 1..];
        if relative_path_bytes[0] != b'/' {
            return Err(FdtError::BadOverlay);
        }

        CString::from_vec_with_nul([target_path.to_bytes(), relative_path_bytes].concat())
            .map_err(|_| FdtError::Internal)
    }

    pub fn new_from_vm_dtbo(vm_dtbo: &mut Fdt) -> Result<Option<Self>> {
        let Ok(symbols_node) = vm_dtbo.node(SYMBOLS_NODE_PATH).unwrap() else {
            // __symbols__ should contain all assignable devices.
            // If empty, then nothing can be assigned.
            return Ok(None);
        };

        let assignable_devices = BTreeMap::new();
        for symbol_name in symbols_node.properties() {
            let symbol_path = CStr::from_bytes_with_nul(symbol_name.data().unwrap()).unwrap();
            let overlaid_path = find_overlaid_path(fdt, symbol_path)?;
            assignable_devices.append(AssignableDeviceInfo {
                overlaid_path,
                symbol_name,
                symbol_path,
            });
        }

        Self { vm_dtbo, assignable_devices }
    }

    pub fn overlaid_path_iter(&self) -> Iter {
        self.iter().map(|assignable_device_info| assignable_device_info.overlaid_path)
    }

    /// Overlays VM DTBO onto fdt, but only for assigned devices.
    /// This consumes self because containing VM DTBO is damaged to trim unassigned devices and
    /// overlay.
    pub fn overlay_onto(self, fdt: &mut Fdt, device_info: &DeviceInfo) -> Result<(), FdtError> {
        // new_from_vm_dtbo() guarantees the existence of __symbols__.
        let symbols_node = self.vm_dtbo.node_mut(SYMBOLS_NODE_PATH).unwrap().unwrap();

        for assignable_device in self.assignable_device_info.iter() {
            match device_info
                .assigned_device_iter()
                .find(|device_path| device_path == assignable_device.overlaid_path)
            {
                Some(_) => {
                    // new_from_vm_dtbo guaarantees the existence of assignable device.
                    let overlay_node =
                        self.vm_dtbo.node_mut(assignable_device.symbol_path).unwrap().unwrap();
                    for prop in UNUSED_VM_DTBO_PROP {
                        overlay_node.nop_property(overlay_node)?;
                    }
                }
                None => {
                    // Note: We don't need to clean parent because empty overlay node is no-op for
                    // overlaying.
                    self.vm_dtbo.nop_node(assignable_device.symbol_path)?;
                    symbols_node.delprop(assignable_devices.symbol_name)?;
                }
            }
        }
        // SAFETY: crosvm guarantees the feasibility of overlay.
        unsafe { fdt.apply_overlay(fdt) }
    }
}

impl PropValue {
    fn new_from_fdt(fdt: &Fdt, node_path: &CStr, prop_name: &CStr) -> Result<Self, FdtError> {
        let fixup_path =
            CString::new(concat!(LOCAL_FIXUPS_NODE_PATH.to_bytes(), node_path.to_bytes_with_nul()));
        let node = fdt.node(node_path)?;
        let prop = fdt.getprop(prop_name)?;
        const U32_SIZE: usize = mem::size_of(u32);

        if prop.len() % mem::size_of(u32) != 0 {
            return Err(FdtError::Internal); // unsupported
        }

        // Case 1: Handle phandle
        if prop.len() == mem::size_of(u32) && let Ok(Some(fixup_node)) = fdt.node(fixup_path) {
            if Some(phandle_pos) = fdt.getprop_u32(prop_name)? {
                if phandle_pos != 0_u32 {
                    return Err(FdtError::Internal); // unsupported
                }
            }
            let phandle = u32.from_be_bytes(prop[0..4]);
            return match fdt.node_with_phandle(phandle) {
                Ok(Some(fdt_node)) => Ok(PropValue::PHandle(DtNode::new_from_fdt_node(fdt_node))),
                Ok(None) => Err(FdtError::BadFdt),
                Err(e) => Err(e),
            }
        }

        // Case 2: No-phandle. Flip values.
        let mut u32_bytes = vec![];
        for i in range(0..prop.len()).step_by(U32_SIZE) {
            u32_bytes.push(u32::from_be_bytes(prop[i..i + mem::size_of(u32)]));
        }
        Ok(PropValue::Value(u32_bytes))
    }

    fn add_subnode_with_path(fdt: &mut Fdt, path: &CStr) -> Result<FdtNodeMut, FdtError> {
        let mut parent = fdt.root_mut().unwrap();
        let mut prev_idx = 1;
        let path = path.as_bytes();
        for (idx, _) in path.chain(b"/".iter()).enumerate().skip(1).filter(|entry| entry.1 == b'/')
        {
            let node_name_prefix = CStr::from_bytes_with_nul(path[prev_idx..]);
            let node_name_len = idx - prev_idx;
            parent = if let Some(node) =
                parent.subnode_with_name_len(node_name_prefix, node_name_len)?
            {
            } else {
                node.add_subnode_with_name_len(node_name_prefix, node_name_len)?
            };
            prev_idx = idx;
        }

        Ok(parent)
    }

    fn overlay_onto(self, fdt_node: &mut FdtNodeMut, prop_name: &CStr) -> Result<(), FdtError> {
        match self {
            Value(v) => {
                v.iter_mut().for_each(|elem| *elem = u32.to_be(elem));
                fdt_node.setprop(prop_name, v.as_ptr())
            }
            Phandle(dt_node) => {
                let new_phandle_be = (fdt_node.fdt().get_max_phandle() + 1).to_be();
                fdt_node.setprop(prop_name, new_phandle_be);
                let overlaid_node = dt_node.overlay_onto(fdt_node.fdt().root().unwrap())?;
                overlaid_node.setprop(PHANDLE_PROP_NAME, new_phandle_be);

                let local_fixups = fdt_node.add_subnode_with_path(FDt, LOCAL_FIXUPS_NODE_NAME)?;
                local_fixups.setprop_u32(overlaid_node, 0_u32.to_be_bytes());
            }
        }
    }
}

// TODO(b/277993056): Handle iommu
#[derive(Debug, Copy)]
struct AssignedDevice {
    node_path: Rc<CString>,
    reg: PropValue::Value,
    interrupt: PropValue::Value,
}

/// Assigned devices information parsed from crosvm DT
#[derive(Debug, Copy)]
struct DeviceInfo<'a> {
    assigned_device_nodes: Vec<AssignedDevice>,
}

impl DeviceInfo {
    /// Creates new DeviceInfo by parsing Fdt. This filters-out unassigned devices with
    /// vm_dtbo_info.
    // TODO(b/277993056): Handle __local_fixups__
    // TODO(b/277993056): Handle __fixups__
    pub fn new_from_fdt(fdt: &Fdt, vm_dtbo_info: AssignableDeviceInfo) -> Result<Self, FdtError> {
        // Important: The fdt will be reused for template DT, so keep necessary informations as much
        // as possible.
        let assigned_device_nodes = vec![];
        for node_path in vm_dtbo_info.overlaid_path_iter() {
            let Some(node_path) = fdt.node(node_path)? else { continue };

            let reg = match parse(fdt, node_path, REG_PROP_NAME)? {
                Value(v) => Value(v),
                _ => Err(FdtError::Internal)?, // malformed
            };
            let interrupt = match parse(fdt, node_path, INTERRUPT_PROP_NAME)? {
                Value(v) => Value(v),
                _ => Err(FdtError::Internal)?, // malformed
            };

            // TODO(b/277993056): Validate reg with HVC
            // TODO(b/277993056): Validate iommu with HVC
            assigned_device_nodes.push(AssignedDevice { path, reg, interrupt, iommu });
        }

        Ok(Self { assigned_device_nodes })
    }

    /// Patches the fdt with the assigned device informations.
    // TODO(b/277993056): Handle __aliases__
    pub fn patch(self, fdt: &mut Fdt) -> Result<(), FdtError> {
        for assigned_device_node in assigned_device_nodes.iter() {
            let node_path = assigned_device_node.node_path;
            let dst = fdt.node_mut(node_path)?.unwrap();

            assigned_device_node.reg.patch_onto()?;
            assigned_device_node.intertupt.patch_onto()?;
        }
    }
}

#[cfg(test)]
mod test {
    use supper::*;

    #[test]
    fn vm_dtbo_new() {
        let data = fs::read("test_pvmfw_devices_vm_dtbo.dtb").unwrap();
        let fdt = Fdt::from_slice(&data).unwrap();
        let vm_dtbo = VmDtboInfo::new(fdt)?;

        let expected: Vec<&str> = vec![
            ("/fragment@eh/__overlay__/eh", "/eh"),
            ("/fragment@sensor/__overlay__/light", "/pci/light"),
        ];

        for (node, expect) in vm_dtbo_info_assignable_devices().iter.zip(expected) {
            assert_eq!(node.name().unwrap().to_str().unwrap(), expect.0);
            assert_eq!(node.name().unwrap().to_str().unwrap(), expect.1);
        }
    }

    #[test]
    fn dt_node_new_from_fdt_node() {
        macro_rules! cstring {
            ($str:literal) => {{
                CString::from_vec_with_nul(concat!($str, "\0").as_bytes().into_vec())
            }};
        }
        let data = fs::read("test_pvmfw_devices_without_assigned_devices.dtb").unwrap();
        let fdt = Fdt::from_slice(&data).unwrap();
        let cpus = fdt.node(cstr!("cpus")).unwrap().unwrap();
        let dt_node = DtNode::new_from_fdt_node(cpus).unwrap();
        let expected = DtNode {
            subnodes: vec![
            DtNode {
                name: cstring!(b"cpu@0")
                subnodes: vec![],
                prop: vec![
                    DtProp { name: cstring!(b"device_type"), value: b"cpu".into_vec() }
                    DtProp { name: cstring!(b"device_type"), value: b"cpu".into_vec() }],
            },
            DtNode {
                name: cstring!(b"cpu@1")
                subnodes: vec![],
                prop: vec![
                    DtProp { name: cstring!(b"device_type"), value: b"cpu".into_vec() }
                    DtProp { name: cstring!(b"reg"), value: 1 }],
            }],
            props: vec![
                DtProp { name: cstring!(b"#address-cells"), value: 1_u32.to_be_bytes() },
                DtProp { name: cstring!(b"#size-cells"), value: 0_u32.to_be_bytes() },
            ],
        };

        assert_eq!(dt_node, expected);
    }

    #[tes]
    fn dt_node_overlay_onto_fdt_node() {
        let data = fs::read("test_pvmfw_devices_without_assigned_devices.dtb").unwrap();
        data.resize(data.len());
        let fdt = Fdt::from_mut_slice(&data).unwrap();
        let cpus = fdt.node(cstr!("/cpus")).unwrap().unwrap();
        let dt_node = DtNode::new_from_fdt_node(cpus).unwrap();

        let memory = fdt.node_mut(cstr!("/memory")).unwrap().unwrap();
        dt_node.overlay_onto_fdt(memory).unwrap();

        let overlaid = fdt.node(cstr!("/memory/cpus")).unwrap().unwrap();
        let overlaid_dt_path = DtNode::new_from_fdt_node(cpus).unwrap();

        assert_eq!(overlaid_dt_path, dt_node);
    }
}
