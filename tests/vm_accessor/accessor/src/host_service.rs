// Copyright 2024, The Android Open Source Project
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

//! Demo service that runs on the host for the VM payload to use

use anyhow::{anyhow, Result};
use com_android_virt_accessor_demo_host_service::{
    aidl::com::android::virt::accessor_demo::host_service::IAccessorHostService::{
        BnAccessorHostService, BpAccessorHostService, IAccessorHostService,
    },
};
use binder::{self, BinderFeatures, Interface, Strong};
use log::info;
use rpcbinder::RpcServer;
use std::thread;

pub fn register_host_service(cid: i32) -> Result<()> {
    thread::spawn(move || {
        let binder = AccessorHostService::new_binder();
        // Set up a binder RPC service for the client in the VM to directly connect to.
        // This only works right now with permissive sepolicy during this test as virtmgr
        // If/when we allow services to set up their own RpcServers, we hide these details
        // underneath an IServiceManager API for registering the service
        let port = 2323;
        let rpc_server =
            match RpcServer::new_vsock(binder.as_binder(), cid.try_into().unwrap(), port) {
                Ok(server) => server,
                Err(err) => {
                    panic!("Failed to set up RpcServer on cid {cid} and port {port}. {err}");
                }
            };
        info!("joining RpcServer for host_vm rpc service");
        rpc_server.join();
    });

    let binder = AccessorHostService::new_binder();
    // Register a kernel binder instance with servicemanager for virtmgr to delegate to
    let service_name =
        <BpAccessorHostService as IAccessorHostService>::get_descriptor().to_owned() + "/default";
    binder::add_service(&service_name, binder.as_binder())
        .map_err(|e| anyhow!("Failed to register service, service={service_name}, err={e:?}",))?;
    info!("service {service_name} is registered with servicemanager");
    Ok(())
}

struct AccessorHostService {}

impl Interface for AccessorHostService {}

impl AccessorHostService {
    fn new_binder() -> Strong<dyn IAccessorHostService> {
        BnAccessorHostService::new_binder(AccessorHostService {}, BinderFeatures::default())
    }
}

impl IAccessorHostService for AccessorHostService {
    fn add(&self, a: i32, b: i32) -> binder::Result<i32> {
        Ok(a + b)
    }
}
