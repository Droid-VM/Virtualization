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

//! This module contains the structs and functions about the message.

use alloc::vec::Vec;
use core::{marker::PhantomData, mem::size_of};
use zerocopy::{
    byteorder::{LittleEndian, U32},
    AsBytes, FromBytes,
};

#[repr(packed)]
#[derive(AsBytes, FromBytes, Clone, Debug)]
struct MessageHeader {
    /// Length of the message attached to this header.
    message_len: U32<LittleEndian>,
}

/// A trait for a byte-level communication channel.
pub trait ByteChannel<E> {
    /// Receives bytes into a buffer from the channel.
    fn recv_bytes(&mut self, buffer: &mut [u8]) -> Result<usize, E>;

    /// Sends a slice of bytes over the channel.
    fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), E>;
}

/// A structured message communication channel built on top of a `ByteChannel`.
pub struct MessageChannel<'a, C: ByteChannel<E>, E> {
    channel: &'a mut C,
    _err: PhantomData<E>,
}

impl<'a, C: ByteChannel<E>, E> MessageChannel<'a, C, E> {
    /// Creates a new `MessageChannel` using the given `ByteChannel`.
    pub fn new(channel: &'a mut C) -> Self {
        Self { channel, _err: PhantomData }
    }

    /// Receives a structured message from the channel and returns the message without header.
    ///
    /// The received raw message should always have a header including the message length padded
    /// in front. This function blocks the reading until the full length of the message is
    /// received.
    pub fn recv_message(&mut self) -> Result<Vec<u8>, E> {
        const HEADER_LEN: usize = size_of::<MessageHeader>();

        let mut message = Vec::new();
        self.recv_bytes_until_len(&mut message, HEADER_LEN)?;

        // Shouldn't panic, because `message.len() >= HEADER_LEN`.
        let message_header = MessageHeader::read_from_prefix(&message[..]).unwrap();
        let message_len = u32::from(message_header.message_len) as usize;
        message.drain(..HEADER_LEN);

        self.recv_bytes_until_len(&mut message, message_len)?;
        Ok(message)
    }

    fn recv_bytes_until_len(&mut self, res: &mut Vec<u8>, target_len: usize) -> Result<(), E> {
        const MAX_RECV_BUFFER_SIZE_BYTES: usize = 64;
        let mut buffer = [0u8; MAX_RECV_BUFFER_SIZE_BYTES];

        while res.len() < target_len {
            let len = self.channel.recv_bytes(&mut buffer)?;
            if len > 0 {
                res.extend_from_slice(&buffer[0..len]);
            }
        }
        Ok(())
    }

    /// Sends the given bytes with a `MessageHeader` padded in front.
    pub fn send_message(&mut self, bytes: &[u8]) -> Result<(), E> {
        let header = MessageHeader { message_len: u32::try_from(bytes.len()).unwrap().into() };
        self.channel.send_bytes(header.as_bytes())?;
        self.channel.send_bytes(bytes)?;

        Ok(())
    }
}
