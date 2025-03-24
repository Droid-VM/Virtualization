//! Rust backened -- supports both kernel binder and RPC binder

use anyhow::Result;
use binder::{self, BinderFeatures, Interface, Strong};
use clap::{Parser, ValueEnum};
use command_fds::CommandFdExt;
use log::{error, info};
use nix::unistd::write;
use rpcbinder::{FileDescriptorTransportMode, RpcServer, RpcSession};
use rustutils::inherited_fd::take_fd_ownership;
use shared_child::SharedChild;
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::io::{AsFd, AsRawFd, OwnedFd, RawFd};
use std::panic;
use std::process::Command;
use test_binder_aidl::aidl::com::ferrochrome::IInstance::{BnInstance, IInstance};
use test_binder_aidl::aidl::com::ferrochrome::IService::{BnService, IService};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum Mode {
    /// IInstance server
    Instance,
    /// IService server returns stub IInstance
    ServerSelfInstance,
    /// IService server returns IInstance via kernel binder
    ServerKernelInstance,
    /// IService server returns IInstance via RPC binder
    ServerRpcInstance,
}

#[derive(Parser, Debug)]
#[command(
    about = "Runs RPC server if (rpc_server_fd, ready_fd) are given. Otherwiser kernel server"
)]
struct Args {
    /// True for IService, false for IInstance
    #[clap(value_enum)]
    mode: Mode,

    /// File descriptor inherited from the caller to run RpcBinder server on.
    /// This should be one end of a socketpair() compatible with RpcBinder's
    /// UDS bootstrap transport.
    #[clap(long)]
    rpc_server_fd: Option<RawFd>,
    /// File descriptor inherited from the caller to signal RpcBinder server
    /// readiness. This should be one end of pipe() and the caller should be
    /// waiting for HUP on the other end.
    #[clap(long)]
    ready_fd: Option<RawFd>,
}

fn main() {
    // Code snippet from virtmgr/main.rs
    // SAFETY: This is very early in the process. Nobody has taken ownership of the inherited FDs
    // yet.
    unsafe { rustutils::inherited_fd::init_once() }
        .expect("Failed to take ownership of inherited FDs");

    let args = Args::parse();

    android_logger::init_once(
        android_logger::Config::default()
            .with_tag(format!("{:?}", args.mode))
            .with_max_level(log::LevelFilter::Debug),
    );
    panic::set_hook(Box::new(|panic_info| {
        error!("Panic: {}", panic_info);
        std::process::exit(0); // Force panic in thread to quit.
    }));

    let res = if args.rpc_server_fd.is_some() && args.ready_fd.is_some() {
        try_rpc_main(&args)
    } else {
        try_kernel_main(&args)
    };

    if let Err(e) = res {
        error!("failed with {:?}", e);
        std::process::exit(1);
    };
}

fn posix_pipe() -> Result<(OwnedFd, OwnedFd), io::Error> {
    use nix::fcntl::OFlag;
    use nix::unistd::pipe2;

    // Create new POSIX pipe. Make it O_CLOEXEC to align with how Rust creates
    // file descriptors (expected by SharedChild).
    Ok(pipe2(OFlag::O_CLOEXEC)?)
}

fn posix_socketpair() -> Result<(OwnedFd, OwnedFd), io::Error> {
    use nix::sys::socket::{socketpair, AddressFamily, SockFlag, SockType};

    // Create new POSIX socketpair, suitable for use with RpcBinder UDS bootstrap
    // transport. Make it O_CLOEXEC to align with how Rust creates file
    // descriptors (expected by SharedChild).
    Ok(socketpair(AddressFamily::Unix, SockType::Stream, None, SockFlag::SOCK_CLOEXEC)?)
}

// Note: Can't return 'Strong<dyn IBinder>'.
fn spawn_and_connect_rpc() -> Strong<dyn IInstance> {
    // Code snippet from VirtualizationService::new_with_path
    let (wait_fd, ready_fd) = posix_pipe().unwrap();
    let (client_fd, server_fd) = posix_socketpair().unwrap();

    let mut command = Command::new("/data/local/tmp/rustbackend");
    command.arg("instance");
    // Can't use BorrowedFd as it doesn't implement Display
    command.arg("--rpc-server-fd").arg(format!("{}", server_fd.as_raw_fd()));
    command.arg("--ready-fd").arg(format!("{}", ready_fd.as_raw_fd()));
    command.preserved_fds(vec![server_fd, ready_fd]);

    SharedChild::spawn(&mut command).expect("Failed to spawn");

    // Wait for the child to signal that the RpcBinder server is read by closing its end of the
    // pipe. Failing to read (especially EACCESS or EPERM) can happen if the client lacks the
    // MANAGE_VIRTUAL_MACHINE permission. Therefore, such errors are propagated instead of
    // being ignored.
    let _ = File::from(wait_fd).read(&mut [0]).expect("Failed to wait for reply");

    // Code snippet from VirtualizationService::connect()
    let session = RpcSession::new();
    session.set_file_descriptor_transport_mode(FileDescriptorTransportMode::Unix);
    session.set_max_incoming_threads(2);

    // Need explicit cast because Rust fails to autodetect
    session.setup_unix_domain_bootstrap_client(client_fd.as_fd()).unwrap()
}

fn try_kernel_main(args: &Args) -> Result<()> {
    binder::ProcessState::start_thread_pool();

    let name = format!("com.ferrochrome.rustbackend.{:?}", args.mode);
    let service = match args.mode {
        Mode::Instance => MyInstance::new_binder().as_binder(),
        _ => MyService::new_binder(&args.mode).as_binder(),
    };
    binder::add_service(&name[..], service).expect("Failed to register kernel service");

    info!("Rust kernel server is starting..");
    binder::ProcessState::join_thread_pool();

    // Unreachable code
    Ok(())
}

fn try_rpc_main(args: &Args) -> Result<()> {
    info!("Starting RPC server.. {args:?}");

    let rpc_server_fd = take_fd_ownership(args.rpc_server_fd.unwrap())
        .expect("Failed to take ownership of rpc_server_fd");
    let ready_fd =
        take_fd_ownership(args.ready_fd.unwrap()).expect("Failed to take ownership of ready_fd");

    binder::ProcessState::start_thread_pool();

    let service = match args.mode {
        Mode::Instance => MyInstance::new_binder().as_binder(),
        _ => MyService::new_binder(&args.mode).as_binder(),
    };
    let server = RpcServer::new_unix_domain_bootstrap(service, rpc_server_fd)
        .expect("Failed to start RpcServer");
    server.set_supported_file_descriptor_transport_modes(&[FileDescriptorTransportMode::Unix]);

    info!("Started RpcServer. Ready to accept connections");

    // Signal readiness to the caller by closing our end of the pipe.
    write(ready_fd.as_fd(), "o".as_bytes())
        .expect("Failed to write a single character through ready_fd");
    drop(ready_fd);

    // Unreachable code
    server.join();

    info!("(unreachable) Shutting down RPC server");

    Ok(())
}

struct MyService(Mode);

impl Interface for MyService {}

impl MyService {
    fn new_binder(mode: &Mode) -> Strong<dyn IService> {
        BnService::new_binder(MyService(*mode), BinderFeatures::default())
    }
}

impl IService for MyService {
    fn ping(&self) -> binder::Result<bool> {
        info!("binder got ping");
        Ok(true)
    }

    fn create(&self) -> binder::Result<binder::Strong<dyn IInstance>> {
        Ok(match self.0 {
            Mode::Instance => panic!("Unreachable!"),
            Mode::ServerSelfInstance => MyInstance::new_binder(),
            Mode::ServerKernelInstance => binder::get_interface::<dyn IInstance>(
                "com.android.ferrochrome.rustbackend.Instance",
            )
            .unwrap(),
            Mode::ServerRpcInstance => spawn_and_connect_rpc(),
        })
    }
}

struct MyInstance {}

impl Interface for MyInstance {}

impl MyInstance {
    fn new_binder() -> Strong<dyn IInstance> {
        BnInstance::new_binder(MyInstance {}, BinderFeatures::default())
    }
}

impl IInstance for MyInstance {
    fn ping(&self) -> binder::Result<bool> {
        info!("ping");
        Ok(true)
    }
}
