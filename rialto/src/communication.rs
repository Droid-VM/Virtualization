// Copyright 2023, The Android Open Source Project
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

//! Supports for the communication between rialto and host.

use crate::error::Result;
use ciborium_io::{Read, Write};
use core::result;
use log::info;
use service_vm_comm::{Request, Response};
use virtio_drivers::{
    self,
    device::socket::{
        SingleConnectionManager, SocketError, VirtIOSocket, VsockAddr, VsockEventType,
    },
    transport::Transport,
    Hal,
};

pub struct VsockStream<H: Hal, T: Transport> {
    connection_manager: SingleConnectionManager<H, T>,
}

impl<H: Hal, T: Transport> VsockStream<H, T> {
    pub fn new(
        socket_device_driver: VirtIOSocket<H, T>,
        peer_addr: VsockAddr,
    ) -> virtio_drivers::Result<Self> {
        let mut connection_manager = SingleConnectionManager::new(socket_device_driver);
        // Use the same port on rialto and peer for convenience.
        connection_manager.connect(peer_addr, peer_addr.port)?;
        connection_manager.wait_for_connect()?;
        info!("Connected to the peer {peer_addr:?}");

        Ok(Self { connection_manager })
    }

    pub fn read_request(&mut self) -> Result<Request> {
        Ok(ciborium::from_reader(self)?)
    }

    pub fn write_response(&mut self, response: &Response) -> Result<()> {
        Ok(ciborium::into_writer(response, self)?)
    }

    /// Shuts down the data channel.
    pub fn shutdown(&mut self) -> virtio_drivers::Result {
        self.connection_manager.force_close()?;
        info!("Connection shutdown.");
        Ok(())
    }

    fn wait_for_recv(&mut self, buffer: &mut [u8]) -> virtio_drivers::Result<usize> {
        loop {
            match self.connection_manager.wait_for_recv(buffer)?.event_type {
                VsockEventType::Disconnected { .. } => {
                    return Err(SocketError::ConnectionFailed.into())
                }
                VsockEventType::Received { length, .. } => return Ok(length),
                VsockEventType::Connected
                | VsockEventType::ConnectionRequest
                // We can safely ignore `CreditRequest` as `SingleConnectionManager` already maintains the
                // connection information, including the local buffer length. Once we receive a buffer, the
                // connection manager updates the information and sends an update to the peer systematically.
                | VsockEventType::CreditRequest
                // Ignore `CreditUpdate` events since we check the peer's credit before sending the buffer.
                // If there is not enough credit available, we will request an update again later.
                | VsockEventType::CreditUpdate => {}
            }
        }
    }
}

impl<H: Hal, T: Transport> Read for VsockStream<H, T> {
    type Error = virtio_drivers::Error;

    fn read_exact(&mut self, data: &mut [u8]) -> result::Result<(), Self::Error> {
        let mut start = 0;
        while start < data.len() {
            let len = self.wait_for_recv(&mut data[start..])?;
            start += len;
        }
        Ok(())
    }
}

impl<H: Hal, T: Transport> Write for VsockStream<H, T> {
    type Error = virtio_drivers::Error;

    fn write_all(&mut self, data: &[u8]) -> result::Result<(), Self::Error> {
        const RETRY_MAX: usize = 3;

        for _ in 0..RETRY_MAX {
            match self.connection_manager.send(data) {
                Ok(_) => return Ok(()),
                Err(virtio_drivers::Error::SocketDeviceError(
                    SocketError::InsufficientBufferSpaceInPeer,
                )) => {}
                Err(e) => return Err(e),
            }
        }
        Err(SocketError::InsufficientBufferSpaceInPeer.into())
    }

    fn flush(&mut self) -> result::Result<(), Self::Error> {
        Ok(())
    }
}
