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

//! Implementation of the AIDL interface `IServiceManager`.

use servicemanager_aidl::aidl::android::os::IServiceManager::{
    BnServiceManager, IServiceManager, CallerContext::CallerContext
};
use servicemanager_aidl::aidl::android::os::{
    Service::Service, IClientCallback::IClientCallback, IServiceCallback::IServiceCallback, ServiceDebugInfo::ServiceDebugInfo, ConnectionInfo::ConnectionInfo
};
use android_system_virtualmachineservice::aidl::android::system::virtualmachineservice::IVirtualMachineService::IVirtualMachineService;
use anyhow::{Result};
use binder::{Accessor, Interface, BinderFeatures, Strong};
use libc::{AF_VSOCK, sa_family_t, sockaddr_vm};
use log::{error, info};
use rpcbinder::{FileDescriptorTransportMode, RpcServer};
use vsock::VMADDR_CID_HOST;
use rustutils::sockets::android_get_control_socket;

// Name of the socket that libbinder is expecting IServiceManager to be served from
const RPC_SERVICEMANAGER_UDS_NAME: &str = "rpc_servicemanager";

/// Implementation of `IServiceManager`.
struct RpcServiceManager {
    virtual_machine_service: Strong<dyn IVirtualMachineService>,
}

impl IServiceManager for RpcServiceManager {
    fn getService(&self, _name: &str) -> binder::Result<Option<binder::SpIBinder>> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn getService2(&self, name: &str) -> binder::Result<Service> {
        let vm_service = self.virtual_machine_service.clone();

        let get_connection_info = move |inst: &str| {
            let connection_info = vm_service.getServiceConnectionInfo(inst).unwrap();
            let addr = sockaddr_vm {
                svm_family: AF_VSOCK as sa_family_t,
                svm_reserved1: 0,
                svm_port: connection_info.port as u32,
                svm_cid: VMADDR_CID_HOST,
                svm_zero: [0u8; 4],
            };
            Some(binder::ConnectionInfo::Vsock(addr))
        };

        let accessor = Accessor::new(name, get_connection_info);

        Ok(Service::Accessor(accessor.as_binder()))
    }
    fn checkService(&self, _name: &str) -> binder::Result<Option<binder::SpIBinder>> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn checkService2(&self, _name: &str) -> binder::Result<Service> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn addService(
        &self,
        _name: &str,
        _service: &binder::SpIBinder,
        _allow_isolated: bool,
        _dump_priority: i32,
    ) -> binder::Result<()> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn listServices(&self, _dump_priority: i32) -> binder::Result<Vec<String>> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn registerForNotifications(
        &self,
        _name: &str,
        _callback: &binder::Strong<dyn IServiceCallback>,
    ) -> binder::Result<()> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn unregisterForNotifications(
        &self,
        _name: &str,
        _callback: &binder::Strong<dyn IServiceCallback>,
    ) -> binder::Result<()> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn isDeclared(&self, _name: &str) -> binder::Result<bool> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn getDeclaredInstances(&self, _iface: &str) -> binder::Result<Vec<String>> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn updatableViaApex(&self, _name: &str) -> binder::Result<Option<String>> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn getUpdatableNames(&self, _apex_name: &str) -> binder::Result<Vec<String>> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn getConnectionInfo(&self, _name: &str) -> binder::Result<Option<ConnectionInfo>> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn registerClientCallback(
        &self,
        _name: &str,
        _service: &binder::SpIBinder,
        _callback: &binder::Strong<dyn IClientCallback>,
    ) -> binder::Result<()> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn tryUnregisterService(
        &self,
        _name: &str,
        _service: &binder::SpIBinder,
    ) -> binder::Result<()> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn getServiceDebugInfo(&self) -> binder::Result<Vec<ServiceDebugInfo>> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
    fn checkServiceAccess(&self, _: &CallerContext, _: &str, _: &str) -> binder::Result<bool> {
        Err(binder::ExceptionCode::UNSUPPORTED_OPERATION.into())
    }
}

impl Interface for RpcServiceManager {}

impl RpcServiceManager {
    /// Creates a new `RpcServiceManager` instance from the `IServiceManager` reference.
    fn new(vm_service: Strong<dyn IVirtualMachineService>) -> RpcServiceManager {
        Self { virtual_machine_service: vm_service }
    }
}

/// Registers the `IServiceManager` service.
pub(crate) fn register_rpc_servicemanager(
    vm_service: Strong<dyn IVirtualMachineService>,
) -> Result<()> {
    let rpc_servicemanager_binder =
        BnServiceManager::new_binder(RpcServiceManager::new(vm_service), BinderFeatures::default());

    let servicemanager_fd = android_get_control_socket(RPC_SERVICEMANAGER_UDS_NAME)?;
    let server =
        RpcServer::new_bound_socket(rpc_servicemanager_binder.as_binder(), servicemanager_fd)?;
    // Required for the FD being passed through libbinder's accessor binder
    server.set_supported_file_descriptor_transport_modes(&[FileDescriptorTransportMode::Unix]);

    info!("The RPC server '{}' is running.", RPC_SERVICEMANAGER_UDS_NAME);
    if let Err(e) = rustutils::system_properties::write("servicemanager.ready", "true") {
        error!("failed to set ro.servicemanager.ready {:?}", e);
    }

    // Move server reference into a background thread and run it forever.
    std::thread::spawn(move || {
        server.join();
    });
    Ok(())
}
