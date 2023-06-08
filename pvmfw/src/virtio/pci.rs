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

//! Functions to scan the PCI bus for VirtIO devices.

use log::debug;
use virtio_drivers::{
    device::blk,
    transport::{
        pci::{
            bus::{BusDeviceIterator, PciRoot},
            virtio_device_type, PciTransport,
        },
        DeviceType, Transport,
    },
};
use vmbase::virtio::HalImpl;

pub type VirtIOBlk = blk::VirtIOBlk<HalImpl, PciTransport>;

pub struct VirtIOBlkIterator<'a> {
    pci_root: &'a mut PciRoot,
    bus: BusDeviceIterator,
}

impl<'a> VirtIOBlkIterator<'a> {
    pub fn new(pci_root: &'a mut PciRoot) -> Self {
        let bus = pci_root.enumerate_bus(0);
        Self { pci_root, bus }
    }
}

impl<'a> Iterator for VirtIOBlkIterator<'a> {
    type Item = VirtIOBlk;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (device_function, info) = self.bus.next()?;
            let (status, command) = self.pci_root.get_status_command(device_function);
            debug!(
                "Found PCI device {} at {}, status {:?} command {:?}",
                info, device_function, status, command
            );

            let Some(virtio_type) = virtio_device_type(&info) else {
                continue;
            };
            debug!("  VirtIO {:?}", virtio_type);

            let mut transport =
                PciTransport::new::<HalImpl>(self.pci_root, device_function).unwrap();
            debug!(
                "Detected virtio PCI device with device type {:?}, features {:#018x}",
                transport.device_type(),
                transport.read_device_features(),
            );

            if virtio_type == DeviceType::Block {
                return Some(Self::Item::new(transport).expect("failed to create blk driver"));
            }
        }
    }
}
