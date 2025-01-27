// Copyright 2024 The Android Open Source Project
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

//! Tests running a VM with LLNDK

use anyhow::{bail, ensure, Context, Result};
use log::info;
use nix::poll::poll;
use nix::poll::PollFd;
use nix::poll::PollFlags;
use nix::poll::PollTimeout;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::os::fd::AsFd;
use std::os::fd::IntoRawFd;
use std::time::Duration;
use vsock::{VsockListener, VsockStream, VMADDR_CID_HOST};

use avf_bindgen::*;
use service_vm_comm::{Request, Response, ServiceVmRequest, VmType};

const VM_MEMORY_MB: i32 = 16;
const WRITE_BUFFER_CAPACITY: usize = 512;

const LISTEN_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_TIMEOUT: timespec = timespec { tv_sec: 10, tv_nsec: 0 };

/// Processes the request in the service VM.
fn process_request(vsock_stream: &mut VsockStream, request: Request) -> Result<Response> {
    write_request(vsock_stream, &ServiceVmRequest::Process(request))?;
    read_response(vsock_stream)
}

/// Sends the request to the service VM.
fn write_request(vsock_stream: &mut VsockStream, request: &impl serde::Serialize) -> Result<()> {
    let mut buffer = BufWriter::with_capacity(WRITE_BUFFER_CAPACITY, vsock_stream);
    ciborium::into_writer(request, &mut buffer)?;
    buffer.flush().context("Failed to flush the buffer")?;
    Ok(())
}

/// Reads the response from the service VM.
fn read_response<T: serde::de::DeserializeOwned>(vsock_stream: &mut VsockStream) -> Result<T> {
    ciborium::from_reader(vsock_stream).context("Failed to read the response from the service VM")
}

fn listen_from_guest(port: u32) -> Result<VsockStream> {
    let vsock_listener =
        VsockListener::bind_with_cid_port(VMADDR_CID_HOST, port).context("Failed to bind vsock")?;
    let mut pollfds = [PollFd::new(vsock_listener.as_fd(), PollFlags::POLLIN)];
    poll(&mut pollfds[..], PollTimeout::try_from(LISTEN_TIMEOUT).unwrap())
        .expect("Failed to poll vsock listener");
    if !pollfds[0].revents().unwrap_or(PollFlags::empty()).contains(PollFlags::POLLIN) {
        bail!("Timeout while listening");
    }
    let (vsock_stream, _peer_addr) = vsock_listener.accept().context("Failed to accept")?;
    Ok(vsock_stream)
}

fn run_rialto(protected_vm: bool) -> Result<()> {
    let kernel_file =
        File::open("/data/local/tmp/rialto.bin").context("Failed to open kernel file")?;
    let kernel_fd = kernel_file.into_raw_fd();

    // SAFETY: AVirtualMachineRawConfig_create() isn't unsafe but rust_bindgen forces it to be seen
    // as unsafe
    let config = unsafe { AVirtualMachineRawConfig_create() };

    info!("raw config created");

    // SAFETY: config is the only reference to a valid object
    unsafe {
        AVirtualMachineRawConfig_setName(config, c"vts_libavf_test_rialto".as_ptr());
        AVirtualMachineRawConfig_setKernel(config, kernel_fd);
        AVirtualMachineRawConfig_setProtectedVm(config, protected_vm);
        AVirtualMachineRawConfig_setMemoryMiB(config, VM_MEMORY_MB);
    }

    let mut vm = std::ptr::null_mut();
    let mut service = std::ptr::null_mut();

    ensure!(
        // SAFETY: &mut service is a valid pointer to *AVirtualizationService
        unsafe { AVirtualizationService_create(&mut service, false) } == 0,
        "AVirtualizationService_create failed"
    );

    scopeguard::defer! {
        // SAFETY: service is a valid pointer to AVirtualizationService
        unsafe { AVirtualizationService_destroy(service); }
    }

    ensure!(
        // SAFETY: &mut vm is a valid pointer to *AVirtualMachine
        unsafe {
            AVirtualMachine_createRaw(
                service, config, -1, // console_in
                -1, // console_out
                -1, // log
                &mut vm,
            )
        } == 0,
        "AVirtualMachine_createRaw failed"
    );

    scopeguard::defer! {
        // SAFETY: vm is a valid pointer to AVirtualMachine
        unsafe { AVirtualMachine_destroy(vm); }
    }

    info!("vm created");

    let vm_type = if protected_vm { VmType::ProtectedVm } else { VmType::NonProtectedVm };

    let listener_thread = std::thread::spawn(move || listen_from_guest(vm_type.port()));

    // SAFETY: vm is the only reference to a valid object
    unsafe {
        AVirtualMachine_start(vm);
    }

    info!("VM started");

    let mut vsock_stream = listener_thread.join().unwrap()?;
    vsock_stream.set_read_timeout(Some(READ_TIMEOUT))?;
    vsock_stream.set_write_timeout(Some(WRITE_TIMEOUT))?;

    info!("client connected");

    let request_data = vec![1, 2, 3, 4, 5];
    let expected_data = vec![5, 4, 3, 2, 1];
    let response = process_request(&mut vsock_stream, Request::Reverse(request_data))
        .context("Failed to process request")?;
    let Response::Reverse(reversed_data) = response else {
        bail!("Expected Response::Reverse but was {response:?}");
    };
    ensure!(reversed_data == expected_data, "Expected {expected_data:?} but was {reversed_data:?}");

    info!("request processed");

    write_request(&mut vsock_stream, &ServiceVmRequest::Shutdown)
        .context("Failed to send shutdown")?;

    info!("shutdown sent");

    let mut stop_reason = AVirtualMachineStopReason::AVIRTUAL_MACHINE_UNRECOGNISED;
    ensure!(
        // SAFETY: vm is the only reference to a valid object
        unsafe { AVirtualMachine_waitForStop(vm, &STOP_TIMEOUT, &mut stop_reason) },
        "AVirtualMachine_waitForStop failed"
    );

    info!("stopped");

    Ok(())
}

#[test]
fn test_run_rialto_protected() -> Result<()> {
    if hypervisor_props::is_protected_vm_supported()? {
        run_rialto(true /* protected_vm */)
    } else {
        info!("pVMs are not supported on device. skipping test");
        Ok(())
    }
}

#[test]
fn test_run_rialto_non_protected() -> Result<()> {
    if hypervisor_props::is_vm_supported()? {
        run_rialto(false /* protected_vm */)
    } else {
        info!("non-pVMs are not supported on device. skipping test");
        Ok(())
    }
}

fn run_testos(protected_vm: bool) -> Result<()> {
    let kernel_file =
        File::open("/data/local/tmp/testos.bin").context("Failed to open kernel file")?;
    let kernel_fd = kernel_file.into_raw_fd();

    // SAFETY: AVirtualMachineRawConfig_create() isn't unsafe but rust_bindgen forces it to be seen
    // as unsafe
    let config = unsafe { AVirtualMachineRawConfig_create() };

    info!("raw config created");

    // SAFETY: config is the only reference to a valid object
    unsafe {
        AVirtualMachineRawConfig_setName(config, c"vts_libavf_test_testos".as_ptr());
        AVirtualMachineRawConfig_setKernel(config, kernel_fd);
        AVirtualMachineRawConfig_setProtectedVm(config, protected_vm);
        AVirtualMachineRawConfig_setMemoryMiB(config, VM_MEMORY_MB);
    }

    // DO NOT SUBMIT: Should crosvm require this to be sealed? Or, maybe do the sealing itself?
    let custom_backing_range = (0x8040_0000_u64, 0x8040_4000_u64);
    let mut custom_backing_file: File = nix::sys::memfd::memfd_create(
        c"custom_backing_file",
        nix::sys::memfd::MemFdCreateFlag::MFD_CLOEXEC,
    )
    .context("memfd_create failed")?
    .into();
    custom_backing_file
        .write_all(&vec![0xAA_u8; (custom_backing_range.1 - custom_backing_range.0) as usize])
        .unwrap();
    ensure!(
        // SAFETY: TODO
        unsafe {
            AVirtualMachineRawConfig_addCustomMemoryBackingFile(
                config,
                custom_backing_file.into_raw_fd(),
                custom_backing_range.0,
                custom_backing_range.1,
            )
        } == 0,
        "AVirtualMachineRawConfig_addCustomMemoryBackingFile failed"
    );

    let mut vm = std::ptr::null_mut();
    let mut service = std::ptr::null_mut();

    ensure!(
        // SAFETY: &mut service is a valid pointer to *AVirtualizationService
        unsafe { AVirtualizationService_create(&mut service, false) } == 0,
        "AVirtualizationService_create failed"
    );

    scopeguard::defer! {
        // SAFETY: service is a valid pointer to AVirtualizationService
        unsafe { AVirtualizationService_destroy(service); }
    }

    ensure!(
        // SAFETY: &mut vm is a valid pointer to *AVirtualMachine
        unsafe {
            AVirtualMachine_createRaw(
                service, config, -1, // console_in
                -1, // console_out
                -1, // log
                &mut vm,
            )
        } == 0,
        "AVirtualMachine_createRaw failed"
    );

    scopeguard::defer! {
        // SAFETY: vm is a valid pointer to AVirtualMachine
        unsafe { AVirtualMachine_destroy(vm); }
    }

    info!("vm created");

    let listener_thread = std::thread::spawn(move || listen_from_guest(8888));

    // SAFETY: vm is the only reference to a valid object
    unsafe {
        AVirtualMachine_start(vm);
    }

    info!("VM started");

    let mut vsock_stream = listener_thread.join().unwrap()?;
    vsock_stream.set_read_timeout(Some(READ_TIMEOUT))?;
    vsock_stream.set_write_timeout(Some(WRITE_TIMEOUT))?;

    info!("client connected");

    write_request(
        &mut vsock_stream,
        &testos_protocol::Request::ReadRange(custom_backing_range.0 as usize, 16),
    )?;
    match read_response(&mut vsock_stream)? {
        testos_protocol::Response::Bytes(b) => {
            assert_eq!(b, vec![0xAA; 16]);
        }
    }

    info!("request processed");

    write_request(&mut vsock_stream, &testos_protocol::Request::Shutdown)
        .context("Failed to send shutdown")?;

    info!("shutdown sent");

    let mut stop_reason = AVirtualMachineStopReason::AVIRTUAL_MACHINE_UNRECOGNISED;
    ensure!(
        // SAFETY: vm is the only reference to a valid object
        unsafe { AVirtualMachine_waitForStop(vm, &STOP_TIMEOUT, &mut stop_reason) },
        "AVirtualMachine_waitForStop failed"
    );

    info!("stopped");

    Ok(())
}

#[test]
fn test_run_testos_non_protected() -> Result<()> {
    if !hypervisor_props::is_vm_supported()? {
        info!("non-pVMs are not supported on device. skipping test");
        return Ok(());
    }
    run_testos(false /* protected_vm */)
}
