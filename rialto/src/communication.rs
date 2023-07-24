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
use log::{info, warn};
use service_vm_comm::{Request, Response};
use virtio_drivers::{
    self,
    device::socket::{
        SocketError, VirtIOSocket, VsockAddr, VsockConnectionManager, VsockEvent, VsockEventType,
    },
    transport::Transport,
    Hal,
};

pub struct VsockStream<H: Hal, T: Transport> {
    connection_manager: VsockConnectionManager<H, T>,
    /// Peer address. The same port is used on rialto and peer for convenience.
    peer_addr: VsockAddr,
}

impl<H: Hal, T: Transport> VsockStream<H, T> {
    pub fn new(
        socket_device_driver: VirtIOSocket<H, T>,
        peer_addr: VsockAddr,
    ) -> virtio_drivers::Result<Self> {
        let mut vsock_stream = Self {
            connection_manager: VsockConnectionManager::new(socket_device_driver),
            peer_addr,
        };
        vsock_stream.connect()?;
        Ok(vsock_stream)
    }

    fn connect(&mut self) -> virtio_drivers::Result {
        self.connection_manager.connect(self.peer_addr, self.peer_addr.port)?;
        self.wait_for_connect()?;
        info!("Connected to the peer {:?}", self.peer_addr);
        Ok(())
    }

    fn wait_for_connect(&mut self) -> virtio_drivers::Result {
        loop {
            let event = self.connection_manager.wait_for_event()?;
            if !self.matches_peer_address(&event) {
                warn!("Received event from the wrong peer: {event:?}");
                continue;
            }
            match event.event_type {
                VsockEventType::Connected => return Ok(()),
                VsockEventType::Disconnected { .. } => {
                    return Err(SocketError::ConnectionFailed.into())
                }
                VsockEventType::Received { .. } => return Err(SocketError::InvalidOperation.into()),
                VsockEventType::ConnectionRequest
                | VsockEventType::CreditRequest
                | VsockEventType::CreditUpdate => {}
            }
        }
    }

    /// Returns whether the event matches the given peer address.
    fn matches_peer_address(&self, event: &VsockEvent) -> bool {
        event.source == self.peer_addr && event.destination.port == self.peer_addr.port
    }

    pub fn read_request(&mut self) -> Result<Request> {
        Ok(ciborium::from_reader(self)?)
    }

    pub fn write_response(&mut self, response: &Response) -> Result<()> {
        Ok(ciborium::into_writer(response, self)?)
    }

    /// Shuts down the data channel.
    pub fn shutdown(&mut self) -> virtio_drivers::Result {
        self.connection_manager.force_close(self.peer_addr, self.peer_addr.port)?;
        info!("Connection shutdown.");
        Ok(())
    }

    fn recv(&mut self, buffer: &mut [u8]) -> virtio_drivers::Result<usize> {
        let length = self.wait_for_recv()?;
        let read_length =
            self.connection_manager.recv(self.peer_addr, self.peer_addr.port, buffer)?;
        assert_eq!(length, read_length);
        Ok(length)
    }

    fn wait_for_recv(&mut self) -> virtio_drivers::Result<usize> {
        loop {
            let event = self.connection_manager.wait_for_event()?;
            if !self.matches_peer_address(&event) {
                warn!("Received event from the wrong peer: {event:?}");
                continue;
            }
            match event.event_type {
                VsockEventType::Disconnected { .. } => {
                    return Err(SocketError::ConnectionFailed.into())
                }
                VsockEventType::Received { length, .. } => return Ok(length),
                VsockEventType::Connected
                | VsockEventType::ConnectionRequest
                // We can safely ignore `CreditRequest` as `SingleConnectionManager` already
                // maintains the connection information, including the local buffer length.
                // Once we receive a buffer, the connection manager updates the information
                // and sends an update to the peer systematically.
                | VsockEventType::CreditRequest
                // Ignore `CreditUpdate` events since we check the peer's credit before sending
                // the buffer.
                // If there is not enough credit available, we will request an update again later.
                | VsockEventType::CreditUpdate => {}
            }
        }
    }

    fn send(&mut self, buffer: &[u8]) -> virtio_drivers::Result {
        self.connection_manager.send(self.peer_addr, self.peer_addr.port, buffer)
    }
}

impl<H: Hal, T: Transport> Read for VsockStream<H, T> {
    type Error = virtio_drivers::Error;

    fn read_exact(&mut self, data: &mut [u8]) -> result::Result<(), Self::Error> {
        let mut start = 0;
        while start < data.len() {
            let len = self.recv(&mut data[start..])?;
            start += len;
        }
        Ok(())
    }
}

impl<H: Hal, T: Transport> Write for VsockStream<H, T> {
    type Error = virtio_drivers::Error;

    fn write_all(&mut self, data: &[u8]) -> result::Result<(), Self::Error> {
        self.send(data)
    }

    fn flush(&mut self) -> result::Result<(), Self::Error> {
        Ok(())
    }
}
