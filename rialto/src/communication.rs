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

use crate::error::{Error, Result};
use log::{error, info};
use virtio_drivers::{
    self,
    device::socket::{SingleConnectionManager, VirtIOSocket, VsockAddr, VsockEventType},
    transport::Transport,
    Hal,
};

const MAX_RECV_BUFFER_SIZE_BYTES: usize = 256;

pub struct DataChannel<H: Hal, T: Transport> {
    connection_manager: SingleConnectionManager<H, T>,
}

impl<H: Hal, T: Transport> From<VirtIOSocket<H, T>> for DataChannel<H, T> {
    fn from(socket_device_driver: VirtIOSocket<H, T>) -> Self {
        Self { connection_manager: SingleConnectionManager::new(socket_device_driver) }
    }
}

impl<H: Hal, T: Transport> DataChannel<H, T> {
    /// Connects to the given destination.
    pub fn connect(&mut self, destination: VsockAddr) -> virtio_drivers::Result {
        self.connection_manager.connect(destination, destination.port)?;
        self.connection_manager.wait_for_connect()?;
        info!("Connected to the destination {destination:?}");
        Ok(())
    }

    /// Processes the received requests and sends back a reply.
    pub fn process_recv(&mut self) -> Result<()> {
        let mut buffer = [0u8; MAX_RECV_BUFFER_SIZE_BYTES];

        // Buffer too short is handled by the socket device driver.
        let recv_event = self.connection_manager.wait_for_recv(&mut buffer)?;
        let VsockEventType::Received { length, .. } = recv_event.event_type else {
            error!("Received unexpected socket event {:?}", recv_event);
            return Err(Error::ReceivingDataFailed);
        };
        // TODO(b/291732060): Implement the communication protocol.
        // Just reverse the received message for now.
        buffer[..length].reverse();
        self.connection_manager.send(&buffer[..length])?;
        Ok(())
    }

    /// Shuts down the data channel.
    pub fn shutdown(&mut self) -> virtio_drivers::Result {
        self.connection_manager.shutdown()?;
        info!("Connection shutdown.");
        Ok(())
    }
}
