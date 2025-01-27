// Copyright 2025, The Android Open Source Project
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

//! Test OS that is meant to be loaded into AVF and then sent requests to probe the VM environment
//! from the guest's perspective.

#![no_main]
#![no_std]

mod communication;
mod exceptions;

extern crate alloc;

use alloc::vec::Vec;
use communication::VsockStream;
use libfdt::Fdt;
use log::debug;
use log::info;
use log::LevelFilter;
use virtio_drivers::device::socket::VsockAddr;
use virtio_drivers::device::socket::VMADDR_CID_HOST;
use virtio_drivers::transport::DeviceType;
use virtio_drivers::transport::Transport;
use vmbase::configure_heap;
use vmbase::fdt::pci::PciInfo;
use vmbase::generate_image_header;
use vmbase::layout::crosvm::FDT_MAX_SIZE;
use vmbase::main;
use vmbase::memory::map_rodata;
use vmbase::memory::SIZE_64KB;
use vmbase::virtio::pci;
use vmbase::virtio::pci::PciTransportIterator;
use vmbase::virtio::pci::VirtIOSocket;
use vmbase::virtio::HalImpl;

generate_image_header!();
main!(main);
configure_heap!(SIZE_64KB);

fn main(arg0: u64, arg1: u64, arg2: u64, arg3: u64) {
    log::set_max_level(LevelFilter::Debug);

    info!("testos started");
    info!("x0={:#018x}, x1={:#018x}, x2={:#018x}, x3={:#018x}", arg0, arg1, arg2, arg3);

    let fdt_addr = usize::try_from(arg0).unwrap();
    map_rodata(fdt_addr, FDT_MAX_SIZE.try_into().unwrap()).unwrap();
    // SAFETY: The DTB range is valid, readable memory, and we don't construct any aliases to it.
    let fdt = unsafe { core::slice::from_raw_parts(fdt_addr as *const u8, FDT_MAX_SIZE) };
    let fdt = Fdt::from_slice(fdt).unwrap();

    let pci_info = PciInfo::from_fdt(fdt).expect("PciInfo::from_fdt failed");
    debug!("PCI: {pci_info:#x?}");
    let mut pci_root = pci::initialize(pci_info).expect("pci::initialize failed");

    let socket_device = VirtIOSocket::<HalImpl>::new(
        PciTransportIterator::<HalImpl, _>::new(&mut pci_root)
            .find(|t| DeviceType::Socket == t.device_type())
            .expect("Missing virtio-vsock device"),
    )
    .expect("VirtIOSocket::new failed");
    debug!("Found socket device: guest cid = {:?}", socket_device.guest_cid());

    let peer_addr = VsockAddr { cid: VMADDR_CID_HOST, port: 8888 };
    let mut vsock_stream =
        VsockStream::new(socket_device, peer_addr).expect("VsockStream::new failed");
    loop {
        let request: testos_protocol::Request =
            ciborium::from_reader(&mut vsock_stream).expect("failed to read request");
        match request {
            testos_protocol::Request::ReadRange(start, size) => {
                // For now, we only allow reads to previously unmapped regions.
                map_rodata(start, size.try_into().unwrap())
                    .expect("failed to map ReadRange request");
                // SAFETY: `map_rodata` just make the range readable and verified the it hadn't
                // been mapped yet, so there should be no aliases.
                let bytes = unsafe { core::slice::from_raw_parts(start as *const u8, size) };
                let response = testos_protocol::Response::Bytes(Vec::from(bytes));
                ciborium::into_writer(&response, &mut vsock_stream)
                    .expect("failed to write response");
            }
            testos_protocol::Request::Shutdown => break,
        }
    }
    vsock_stream.shutdown().expect("failed to shutdown vsock");
}
