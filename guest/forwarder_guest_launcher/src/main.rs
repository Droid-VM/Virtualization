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

//! Launcher of forwarder_guest

use anyhow::Context;
use clap::Parser;
use debian_service::debian_service_client::DebianServiceClient;
use debian_service::{QueueOpeningRequest, ReportVmActivePortsRequest};
use log::{debug, error};
use tokio::process::Command;
use tokio::time::{sleep, Duration};
use tonic::transport::Endpoint;
use tonic::Request;

mod debian_service {
    tonic::include_proto!("com.android.virtualization.vmlauncher.proto");
}

#[derive(Parser)]
/// Flags for running command
pub struct Args {
    /// Host IP address
    #[arg(long)]
    #[arg(alias = "host")]
    host_addr: String,
    /// grpc port number
    #[arg(long)]
    #[arg(alias = "grpc_port")]
    grpc_port: String,
}

async fn process_forwarding_request_queue(
    host_addr: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let channel = Endpoint::from_shared(host_addr)?.connect().await?;
    let mut client = DebianServiceClient::new(channel);
    let cid = vsock::get_local_cid().context("Failed to get CID of VM")?;
    let mut res_stream = client
        .open_forwarding_request_queue(Request::new(QueueOpeningRequest { cid: cid as i32 }))
        .await?
        .into_inner();

    while let Some(response) = res_stream.message().await? {
        let tcp_port = i16::try_from(response.guest_tcp_port)
            .context("Failed to convert guest_tcp_port as i16")?;
        let vsock_port = response.vsock_port as u32;

        debug!(
            "executing forwarder_guest with guest_tcp_port: {:?}, vsock_port: {:?}",
            &tcp_port, &vsock_port
        );

        let _ = Command::new("forwarder_guest")
            .arg("--local")
            .arg(format!("127.0.0.1:{}", tcp_port))
            .arg("--remote")
            .arg(format!("vsock:2:{}", vsock_port))
            .spawn();
    }
    Ok(())
}

async fn report_active_ports(host_addr: String) -> Result<(), Box<dyn std::error::Error>> {
    let channel = Endpoint::from_shared(host_addr)?.connect().await?;
    let mut client = DebianServiceClient::new(channel);
    loop {
        let listeners = listeners::get_all()?;
        let listening_ports = listeners
            .iter()
            .map(|x| x.socket)
            .filter(|x| x.is_ipv4())
            .map(|x| x.port().into())
            .filter(|x| *x >= 1024)
            .collect();
        let res = client
            .report_vm_active_ports(Request::new(ReportVmActivePortsRequest {
                ports: listening_ports,
            }))
            .await?
            .into_inner();
        if !res.success {
            error!("Failure response received from the host for reporting active ports");
        }
        sleep(Duration::from_millis(1000)).await;
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    debug!("Starting forwarder_guest_launcher");
    let args = Args::parse();
    let addr = format!("https://{}:{}", args.host_addr, args.grpc_port);

    tokio::select! {
        forwarding_result = process_forwarding_request_queue(addr.clone()) => match forwarding_result {
            Ok(_) => debug!("process_forwarding_request_queue is finished"),
            Err(e) => error!("Error on process_forwarding_request_queue: {:?}", e),
        },
        reporting_ports_result = report_active_ports(addr) => match reporting_ports_result {
            Ok(_) => debug!("report_active_ports is finished"),
            Err(e) => error!("Error on report_active_ports: {:?}", e),
        }
    }
}
