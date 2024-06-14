//! Whatever

use anyhow::Error;
use libc::VMADDR_CID_ANY;
use vsock::VsockListener;

fn main() -> Result<(), Error> {
    env_logger::init();

    let listener = VsockListener::bind_with_cid_port(VMADDR_CID_ANY, 1066)?;
    println!("Listening");

    for stream in listener.incoming() {
        let stream = match stream {
            Err(e) => {
                println!("invalid incoming connection: {e:?}");
                continue;
            }
            Ok(s) => s,
        };

        println!("Got connection from {:?}", stream.peer_addr());
    }

    Ok(())
}
