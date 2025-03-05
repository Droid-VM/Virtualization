use api::debian_service_server::DebianService;
use api::debian_service_server::DebianServiceServer;
use api::StorageBalloonQueueOpeningRequest;
use api::StorageBalloonRequestItem;
use api::*;

use clap::Parser;
pub mod api {
    tonic::include_proto!("com.android.virtualization.terminal.proto");
}
use std::boxed::Box;
use std::pin::Pin;
use tokio_stream::Stream;
use tonic::transport::Server;
use tonic::Response;
use tonic::Status;

#[derive(Parser)]
/// Flags for running command
pub struct Args {
    /// grpc port number
    #[arg(long)]
    #[arg(alias = "grpc_port")]
    grpc_port: String,
}

#[derive(Default)]
struct Daemon {}

type QueueStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

#[tonic::async_trait]
impl DebianService for Daemon {
    type OpenForwardingRequestQueueStream = QueueStream<ForwardingRequestItem>;
    type OpenShutdownRequestQueueStream = QueueStream<ShutdownRequestItem>;
    type OpenStorageBalloonRequestQueueStream = QueueStream<StorageBalloonRequestItem>;

    async fn report_vm_active_ports(
        &self,
        _request: tonic::Request<ReportVmActivePortsRequest>,
    ) -> std::result::Result<tonic::Response<ReportVmActivePortsResponse>, tonic::Status> {
        unimplemented!();
    }
    async fn open_forwarding_request_queue(
        &self,
        _request: tonic::Request<QueueOpeningRequest>,
    ) -> std::result::Result<tonic::Response<Self::OpenForwardingRequestQueueStream>, tonic::Status>
    {
        unimplemented!();
    }
    async fn open_shutdown_request_queue(
        &self,
        _request: tonic::Request<ShutdownQueueOpeningRequest>,
    ) -> std::result::Result<tonic::Response<Self::OpenShutdownRequestQueueStream>, tonic::Status>
    {
        unimplemented!();
    }

    async fn open_storage_balloon_request_queue(
        &self,
        _request: tonic::Request<StorageBalloonQueueOpeningRequest>,
    ) -> std::result::Result<
        tonic::Response<Self::OpenStorageBalloonRequestQueueStream>,
        tonic::Status,
    > {
        let output = async_stream::stream! { // Use async_stream for easier stream creation
            loop {
                let req = StorageBalloonRequestItem {
                    available_bytes: 1024,
                };
                yield Result::<_, Status>::Ok(req);
                println!("sent request: {:?}", req);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await; // Add delay for demonstration
            }
        };
        Ok(Response::new(Box::pin(output))) // as Self::OpenStorageBalloonRequestQueueStream))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("storage_balloon_datemon started");
    let daemon = Daemon::default();
    let addr = format!("127.0.0.1:{}", args.grpc_port);
    println!("addr={:?}", addr);
    Server::builder().add_service(DebianServiceServer::new(daemon)).serve(addr.parse()?).await?;

    Ok(())
}
