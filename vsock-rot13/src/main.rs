//! Test service to listen on a vsock port, and send back any text received encoded with ROT13.

use libc::VMADDR_CID_ANY;
use nix::sys::socket::{SockAddr, VsockAddr};
use std::convert::TryInto;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::str;
use std::thread;
use vsock::VsockListener;

const BUFFER_SIZE: usize = 16384;
const PORT: u32 = 1234;

fn rot13(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if (c.is_ascii_lowercase() && c >= 'n') || (c.is_ascii_uppercase() && c >= 'N') {
                (c as u32 - 13).try_into().unwrap()
            } else if c.is_ascii_alphabetic() {
                (c as u32 + 13).try_into().unwrap()
            } else {
                c
            }
        })
        .collect()
}

fn main() {
    let listener = VsockListener::bind(&SockAddr::Vsock(VsockAddr::new(VMADDR_CID_ANY, PORT)))
        .expect("Failed to bind to port.");
    println!("ROT13 server listening on port {}", PORT);

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                println!("Connection from {}", stream.peer_addr().unwrap());
                thread::spawn(move || {
                    let mut buf = vec![0; BUFFER_SIZE];
                    loop {
                        let read_size = stream.read(&mut buf).expect("Read failed.");
                        if read_size == 0 {
                            break;
                        }

                        let input_string =
                            str::from_utf8(&buf[0..read_size]).expect("Invalid UTF-8 string.");
                        let output_string = rot13(input_string);
                        let output_bytes = output_string.as_bytes();

                        let mut bytes_sent = 0;
                        while bytes_sent < output_bytes.len() {
                            let bytes_written = stream
                                .write(&output_bytes[bytes_sent..])
                                .expect("Write failed.");
                            if bytes_written == 0 {
                                break;
                            }
                            bytes_sent += bytes_written;
                        }
                    }

                    println!("{} disconnected.", stream.peer_addr().unwrap());
                    stream.shutdown(Shutdown::Both).unwrap();
                });
            }
            Err(e) => {
                println!("Error accepting connection: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rot13_empty() {
        assert_eq!(rot13(""), "");
    }

    #[test]
    fn rot13_ascii() {
        assert_eq!(rot13("Hello world."), "Uryyb jbeyq.");
    }
}
