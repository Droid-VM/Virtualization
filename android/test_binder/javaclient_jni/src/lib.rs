//! Library for FFI

use command_fds::CommandFdExt;
use shared_child::SharedChild;
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::io::{AsRawFd, IntoRawFd, OwnedFd, RawFd};
use std::process::Command;

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

/// FFI
///
/// # Safety
/// Actually not unsafe
#[no_mangle]
pub unsafe extern "C" fn connect_rpc() -> RawFd {
    let (wait_fd, ready_fd) = posix_pipe().unwrap();
    let (client_fd, server_fd) = posix_socketpair().unwrap();

    let mut command = Command::new("/data/local/tmp/rustbackend");
    command.arg("server-self-instance");
    command.arg("--rpc-server-fd").arg(format!("{}", server_fd.as_raw_fd()));
    command.arg("--ready-fd").arg(format!("{}", ready_fd.as_raw_fd()));
    command.preserved_fds(vec![server_fd, ready_fd]);

    SharedChild::spawn(&mut command).unwrap();

    // Wait for the child to signal that the RpcBinder server is read by closing its end of the
    // pipe. Failing to read (especially EACCESS or EPERM) can happen if the client lacks the
    // MANAGE_VIRTUAL_MACHINE permission. Therefore, such errors are propagated instead of
    // being ignored.
    let _ = File::from(wait_fd).read(&mut [0]).unwrap();

    client_fd.into_raw_fd()
}
