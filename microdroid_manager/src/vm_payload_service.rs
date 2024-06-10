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

//! Implementation of the AIDL interface `IVmPayloadService`.

use android_system_virtualization_payload::aidl::android::system::virtualization::payload::IVmPayloadService::{
    BnVmPayloadService, IVmPayloadService, VM_PAYLOAD_SERVICE_SOCKET_NAME, AttestationResult::AttestationResult,
    STATUS_FAILED_TO_PREPARE_CSR_AND_KEY
};
use android_system_virtualmachineservice::aidl::android::system::virtualmachineservice::IVirtualMachineService::IVirtualMachineService;
use android_os_accessor::aidl::android::os::IAccessor::{IAccessor, BnAccessor};
use anyhow::{anyhow, Context, Result};
use avflog::LogResult;
use binder::{Interface, BinderFeatures, ExceptionCode, Strong, IntoBinderResult, Status, ParcelFileDescriptor};
use client_vm_csr::{generate_attestation_key_and_csr, ClientVmAttestationData};
use log::info;
use rpcbinder::{RpcServer};
use crate::vm_secret::VmSecret;
use std::os::unix::io::OwnedFd;
use std::os::fd::{FromRawFd, IntoRawFd};
use std::fs::File;
use std::collections::HashMap;
use std::sync::Mutex;
use vsock::{VMADDR_CID_HOST, VsockStream};

/// Implementation of `IVmPayloadService`.
struct VmPayloadService {
    allow_restricted_apis: bool,
    virtual_machine_service: Strong<dyn IVirtualMachineService>,
    secret: VmSecret,
}

struct Accessor {
    virtual_machine_service: Strong<dyn IVirtualMachineService>,
    // The client is going to request multiple FDs for this service.
    // One for the initial connection, and then another when we getRootObject.
    // Just create new connections from the same port?
    connection_info: Mutex<HashMap<String, i32>>,
}

impl IAccessor for Accessor {
    fn connectToRpcSession(&self) -> binder::Result<ParcelFileDescriptor> {
        info!("connectToRpcSession in microdroid_manager starting");
        let name = "android.hardware.light.ILights/default";
        let mut con_info = self.connection_info.lock().unwrap();
        let port = match con_info.get(name) {
            Some(&number) => {
                info!("Already have connection info for this service");
                number
            }
            None => {
                // TODO IAccessor::connectToRpcSession doesn't have "name"?
                // if that's the case, we need getIAccessor to take a name and keep the name: String
                // in each Accesssor instance
                let connection_info =
                    self.virtual_machine_service.getServiceConnectionInfo(name)?;
                info!("connectToRpcSession in microdroid_manager got connection info from virtmg");
                con_info.insert(name.to_string(), connection_info.port);
                connection_info.port
            }
        };

        let fd = VsockStream::connect_with_cid_port(VMADDR_CID_HOST, port.try_into().unwrap())
            .context("Failed to connect")
            .or_service_specific_exception(-1)?;
        info!("connectToRpcSession in microdroid_manager succesfully connected to port!");

        // TODO... from raw to raw? why?
        // https://source.corp.google.com/piper///depot/google3/third_party/cloud_hypervisor/net_util/src/tap.rs;rcl=636185034;l=129
        // SAFETY: yup it's not safe.
        let f = unsafe { File::from_raw_fd(fd.into_raw_fd()) };
        Ok(ParcelFileDescriptor::new(f))
    }
}

impl binder::Interface for Accessor {}

impl IVmPayloadService for VmPayloadService {
    fn notifyPayloadReady(&self) -> binder::Result<()> {
        self.virtual_machine_service.notifyPayloadReady()
    }

    fn getVmInstanceSecret(&self, identifier: &[u8], size: i32) -> binder::Result<Vec<u8>> {
        if !(0..=32).contains(&size) {
            return Err(anyhow!("size {size} not in range (0..=32)"))
                .or_binder_exception(ExceptionCode::ILLEGAL_ARGUMENT);
        }
        let mut instance_secret = vec![0; size.try_into().unwrap()];
        self.secret
            .derive_payload_sealing_key(identifier, &mut instance_secret)
            .context("Failed to derive VM instance secret")
            .with_log()
            .or_service_specific_exception(-1)?;
        Ok(instance_secret)
    }

    fn getDiceAttestationChain(&self) -> binder::Result<Vec<u8>> {
        self.check_restricted_apis_allowed()?;
        if let Some(bcc) = self.secret.dice_artifacts().bcc() {
            Ok(bcc.to_vec())
        } else {
            Err(anyhow!("bcc is none")).or_binder_exception(ExceptionCode::ILLEGAL_STATE)
        }
    }

    fn getDiceAttestationCdi(&self) -> binder::Result<Vec<u8>> {
        self.check_restricted_apis_allowed()?;
        Ok(self.secret.dice_artifacts().cdi_attest().to_vec())
    }

    fn requestAttestation(
        &self,
        challenge: &[u8],
        test_mode: bool,
    ) -> binder::Result<AttestationResult> {
        let ClientVmAttestationData { private_key, csr } =
            generate_attestation_key_and_csr(challenge, self.secret.dice_artifacts())
                .map_err(|e| {
                    Status::new_service_specific_error_str(
                        STATUS_FAILED_TO_PREPARE_CSR_AND_KEY,
                        Some(format!("Failed to prepare the CSR and key pair: {e:?}")),
                    )
                })
                .with_log()?;
        let csr = csr
            .into_cbor_vec()
            .map_err(|e| {
                Status::new_service_specific_error_str(
                    STATUS_FAILED_TO_PREPARE_CSR_AND_KEY,
                    Some(format!("Failed to serialize CSR into CBOR: {e:?}")),
                )
            })
            .with_log()?;
        let cert_chain = self.virtual_machine_service.requestAttestation(&csr, test_mode)?;
        Ok(AttestationResult {
            privateKey: private_key.as_slice().to_vec(),
            certificateChain: cert_chain,
        })
    }

    fn getIAccessor(&self) -> binder::Result<Strong<dyn IAccessor>> {
        Ok(BnAccessor::new_binder(
            Accessor {
                virtual_machine_service: self.virtual_machine_service.clone(),
                connection_info: HashMap::new().into(),
            },
            BinderFeatures::default(),
        ))
    }
}

impl Interface for VmPayloadService {}

impl VmPayloadService {
    /// Creates a new `VmPayloadService` instance from the `IVirtualMachineService` reference.
    fn new(
        allow_restricted_apis: bool,
        vm_service: Strong<dyn IVirtualMachineService>,
        secret: VmSecret,
    ) -> VmPayloadService {
        Self { allow_restricted_apis, virtual_machine_service: vm_service, secret }
    }

    fn check_restricted_apis_allowed(&self) -> binder::Result<()> {
        if self.allow_restricted_apis {
            Ok(())
        } else {
            Err(anyhow!("Use of restricted APIs is not allowed"))
                .with_log()
                .or_binder_exception(ExceptionCode::SECURITY)
        }
    }
}

/// Registers the `IVmPayloadService` service.
pub(crate) fn register_vm_payload_service(
    allow_restricted_apis: bool,
    vm_service: Strong<dyn IVirtualMachineService>,
    secret: VmSecret,
    vm_payload_service_fd: OwnedFd,
) -> Result<()> {
    let vm_payload_binder = BnVmPayloadService::new_binder(
        VmPayloadService::new(allow_restricted_apis, vm_service, secret),
        BinderFeatures::default(),
    );

    let server = RpcServer::new_bound_socket(vm_payload_binder.as_binder(), vm_payload_service_fd)?;
    info!("The RPC server '{}' is running.", VM_PAYLOAD_SERVICE_SOCKET_NAME);

    // Move server reference into a background thread and run it forever.
    std::thread::spawn(move || {
        server.join();
    });
    Ok(())
}
