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

//! This module contains the functions to start, stop and communicate with the
//! Service VM.

use android_system_virtualizationservice::{
    aidl::android::system::virtualizationservice::{
        CpuTopology::CpuTopology, DiskImage::DiskImage,
        IVirtualizationService::IVirtualizationService, Partition::Partition,
        PartitionType::PartitionType, VirtualMachineConfig::VirtualMachineConfig,
        VirtualMachineRawConfig::VirtualMachineRawConfig,
    },
    binder::ParcelFileDescriptor,
};
use anyhow::{anyhow, Context, Result};
use log::info;
use service_vm_comm::{host_port, Request, Response};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Duration;
use vmclient::VmInstance;
use vsock::{VsockListener, VsockStream, VMADDR_CID_HOST};

const VIRT_DATA_DIR: &str = "/data/misc/apexdata/com.android.virt";
const RIALTO_PATH: &str = "/apex/com.android.virt/etc/rialto.bin";
const INSTANCE_IMG_NAME: &str = "service_vm_instance.img";
const INSTANCE_IMG_SIZE_BYTES: i64 = 1 << 20; // 1MB
const MEMORY_MB: i32 = 300;
const WRITE_BUFFER_CAPACITY: usize = 512;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const PROTECTED_VM: bool = true;

/// Service VM.
pub struct ServiceVm {
    vm: VmInstance,
    vsock_stream: VsockStream,
}

impl ServiceVm {
    /// Shuts down the service VM.
    pub fn shutdown(&self) -> Result<()> {
        self.vm
            .wait_for_death_with_timeout(Duration::from_secs(10))
            .ok_or_else(|| anyhow!("Timed out waiting for VM exit"))?;
        info!("Shut down the service VM");
        Ok(())
    }

    /// Processes the request in the service VM.
    pub fn process_request(&mut self, request: &Request) -> Result<Response> {
        self.write_request(request)?;
        self.read_response()
    }

    /// Sends the request to the service VM.
    fn write_request(&mut self, request: &Request) -> Result<()> {
        let mut buffer = BufWriter::with_capacity(WRITE_BUFFER_CAPACITY, &mut self.vsock_stream);
        ciborium::into_writer(request, &mut buffer)?;
        buffer.flush().context("Failed to flush the buffer")?;
        info!("Sent request to the service VM.");
        Ok(())
    }

    /// Reads the response from the service VM.
    fn read_response(&mut self) -> Result<Response> {
        let response: Response = ciborium::from_reader(&mut self.vsock_stream)
            .context("Failed to read the response from the service VM")?;
        info!("Received response from the service VM.");
        Ok(response)
    }
}

/// Starts the service VM and returns its instance.
/// The same instance image is used for different VMs.
/// TODO(b/278858244): Allow only one service VM running at each time.
pub fn start() -> Result<ServiceVm> {
    let vm = vm_instance()?;
    start_vm(vm, PROTECTED_VM)
}

/// Starts the given VM instance and sets up the vsock connection with it.
/// Returns a `ServiceVm` instance.
pub fn start_vm(vm: VmInstance, protected_vm: bool) -> Result<ServiceVm> {
    // Sets up the vsock server on the host.
    let port = host_port(protected_vm);
    let vsock_listener = VsockListener::bind_with_cid_port(VMADDR_CID_HOST, port)?;

    // Starts the service VM.
    vm.start().context("Failed to start service VM")?;
    info!("Service VM started");

    // Accepts the connection from the service VM.
    let vsock_stream =
        vsock_listener.incoming().next().ok_or_else(|| anyhow!("Failed to get vsock_stream"))??;
    info!("Accepted connection {:?}", vsock_stream);
    vsock_stream.set_read_timeout(Some(READ_TIMEOUT))?;
    vsock_stream.set_write_timeout(Some(WRITE_TIMEOUT))?;

    let service_vm = ServiceVm { vm, vsock_stream };
    Ok(service_vm)
}

fn vm_instance() -> Result<VmInstance> {
    let virtmgr = vmclient::VirtualizationService::new().context("Failed to spawn VirtMgr")?;
    let service = virtmgr.connect().context("Failed to connect to VirtMgr")?;
    info!("Connected to VirtMgr for service VM");

    let instance_img = instance_img(service.as_ref())?;
    let writable_partitions = vec![Partition {
        label: "vm-instance".to_owned(),
        image: Some(instance_img),
        writable: true,
    }];
    let rialto = File::open(RIALTO_PATH).context("Failed to open Rialto kernel binary")?;
    let config = VirtualMachineConfig::RawConfig(VirtualMachineRawConfig {
        name: String::from("Service VM"),
        bootloader: Some(ParcelFileDescriptor::new(rialto)),
        disks: vec![DiskImage { image: None, partitions: writable_partitions, writable: true }],
        protectedVm: PROTECTED_VM,
        memoryMib: MEMORY_MB,
        cpuTopology: CpuTopology::ONE_CPU,
        platformVersion: "~1.0".to_string(),
        gdbPort: 0, // No gdb
        ..Default::default()
    });
    let console_out = None;
    let console_in = None;
    let log = None;
    let callback = None;
    VmInstance::create(service.as_ref(), &config, console_out, console_in, log, callback)
        .context("Failed to create service VM")
}

fn instance_img(service: &dyn IVirtualizationService) -> Result<ParcelFileDescriptor> {
    let instance_img_path = Path::new(VIRT_DATA_DIR).join(INSTANCE_IMG_NAME);
    if instance_img_path.exists() {
        // TODO(b/298174584): Try to recover if the service VM is triggered by rkpd.
        return Ok(OpenOptions::new()
            .read(true)
            .write(true)
            .open(instance_img_path)
            .map(ParcelFileDescriptor::new)?);
    }
    let instance_img = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(instance_img_path)
        .map(ParcelFileDescriptor::new)?;
    service.initializeWritablePartition(
        &instance_img,
        INSTANCE_IMG_SIZE_BYTES,
        PartitionType::ANDROID_VM_INSTANCE,
    )?;
    Ok(instance_img)
}
