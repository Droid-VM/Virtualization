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

use anyhow::{anyhow, Context};
use clap::Parser;
use debian_service::debian_service_client::DebianServiceClient;
use debian_service::{QueueOpeningRequest, ReportVmActivePortsRequest};
use log::{debug, error};
use std::collections::HashSet;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::try_join;
use tonic::transport::{Channel, Endpoint};
use tonic::Request;

mod debian_service {
    tonic::include_proto!("com.android.virtualization.vmlauncher.proto");
}

const NON_PREVILEGED_PORT_RANGE_START: i32 = 1024;
const TCPSTATES_COLUMN_IP_TITLE: &str = "IP";
const TCPSTATES_COLUMN_LPORT_TITLE: &str = "LPORT";
const TCPSTATES_COLUMN_NEWSTATE_TITLE: &str = "NEWSTATE";
const TCPSTATES_COLUMN_OLDSTATE_TITLE: &str = "OLDSTATE";
const TCPSTATES_IP_4: &str = "4";
const TCPSTATES_STATE_CLOSE: &str = "CLOSE";
const TCPSTATES_STATE_LISTEN: &str = "LISTEN";

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
    mut client: DebianServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
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
    Err(anyhow!("process_forwarding_request_queue is terminated").into())
}

async fn send_single_active_ports_report(
    listening_ports: HashSet<i32>,
    client: &mut DebianServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let res = client
        .report_vm_active_ports(Request::new(ReportVmActivePortsRequest {
            ports: listening_ports.into_iter().collect(),
        }))
        .await?
        .into_inner();
    if res.success {
        debug!("Successfully reported active ports to the host");
    } else {
        error!("Failure response received from the host for reporting active ports");
    }
    Ok(())
}

async fn report_active_ports(
    mut client: DebianServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::new("script")
        .arg("-q")
        .arg("-c")
        .arg("/usr/sbin/tcpstates-bpfcc -s")
        .stdout(Stdio::piped())
        .spawn()?;
    let stdout = cmd.stdout.take().context("Failed to get stdout of tcpstates")?;
    let mut reader = BufReader::new(stdout).lines();

    let header_line =
        reader.next_line().await?.ok_or(anyhow!("Failed to parse entry line of tcpstates"))?;
    let columns: Vec<_> = header_line.split(',').collect();
    let ip_column_idx = columns
        .iter()
        .position(|x| x == &TCPSTATES_COLUMN_IP_TITLE)
        .ok_or(anyhow!("No IP column in tcpstates header"))?;
    let port_column_idx = columns
        .iter()
        .position(|x| x == &TCPSTATES_COLUMN_LPORT_TITLE)
        .ok_or(anyhow!("No LPORT column in tcpstates header"))?;
    let old_state_column_idx = columns
        .iter()
        .position(|x| x == &TCPSTATES_COLUMN_OLDSTATE_TITLE)
        .ok_or(anyhow!("No OLDSTATE column in tcpstates header"))?;
    let new_state_column_idx = columns
        .iter()
        .position(|x| x == &TCPSTATES_COLUMN_NEWSTATE_TITLE)
        .ok_or(anyhow!("No NEWSTATE column in tcpstates header"))?;
    debug!("Monitoring ports with tcpstate is ready");

    // TODO(b/340126051): Consider using NETLINK_SOCK_DIAG for the optimization.
    let listeners = listeners::get_all()?;
    // TODO(b/340126051): Support distinguished port forwarding for ipv6 as well.
    let mut listening_ports: HashSet<_> = listeners
        .iter()
        .map(|x| x.socket)
        .filter(|x| x.is_ipv4())
        .map(|x| x.port().into())
        .filter(|x| *x >= NON_PREVILEGED_PORT_RANGE_START) // Ignore privileged ports
        .collect();
    send_single_active_ports_report(listening_ports.clone(), &mut client).await?;

    while let Some(line) = reader.next_line().await? {
        let items: Vec<_> = line.split(',').collect();
        if items.get(ip_column_idx) != Some(&TCPSTATES_IP_4) {
            continue;
        }
        match (items.get(old_state_column_idx), items.get(new_state_column_idx)) {
            (Some(&TCPSTATES_STATE_CLOSE), Some(&TCPSTATES_STATE_LISTEN)) => {
                listening_ports.insert(
                    items
                        .get(port_column_idx)
                        .ok_or(anyhow!("Failed to get port"))?
                        .parse()
                        .context("Failed to parse port")?,
                );
            }
            (Some(&TCPSTATES_STATE_LISTEN), Some(&TCPSTATES_STATE_CLOSE)) => {
                listening_ports.remove(
                    &items
                        .get(port_column_idx)
                        .ok_or(anyhow!("Failed to get port"))?
                        .parse()
                        .context("Failed to parse port")?,
                );
            }
            (_, _) => continue,
        }
        send_single_active_ports_report(listening_ports.clone(), &mut client).await?;
    }

    Err(anyhow!("report_active_ports is terminated").into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    debug!("Starting forwarder_guest_launcher");
    let args = Args::parse();
    let addr = format!("https://{}:{}", args.host_addr, args.grpc_port);
    let channel = Endpoint::from_shared(addr)?.connect().await?;
    let client = DebianServiceClient::new(channel);

    try_join!(process_forwarding_request_queue(client.clone()), report_active_ports(client))?;
    Ok(())
}
