//! Whatever

use anyhow::Error;
use libc::VMADDR_CID_HOST;
use vsock::VsockStream;

fn main() -> Result<(), Error> {
    env_logger::init();

    println!("Hello foo");

    let stream = VsockStream::connect_with_cid_port(VMADDR_CID_HOST, 1066)?;

    println!("Connected to {:?}", stream.peer_addr());

    Ok(())
}
