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

//! This module contains the requests and responses definitions exchanged
//! between the host and the service VM.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use core::fmt;
use serde::{de, ser::SerializeSeq, Deserialize, Deserializer, Serialize, Serializer};

/// Represents a request to be sent to the service VM.
///
/// Each request has a corresponding response item.
#[derive(Clone, Debug)]
pub enum Request {
    /// Reverse the order of the bytes in the provided byte array.
    /// Currently this is only used for testing.
    Reverse(Vec<u8>),
}

/// Represents a response to a request sent to the service VM.
///
/// Each response corresponds to a specific request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    /// Reverse the order of the bytes in the provided byte array.
    Reverse(Vec<u8>),
}

struct TypeCodes;

impl TypeCodes {
    const REVERSE: u8 = 1;
}

impl Serialize for Request {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(2))?;
        match self {
            Request::Reverse(arr) => {
                seq.serialize_element(&TypeCodes::REVERSE)?;
                seq.serialize_element(arr)?;
            }
        }
        seq.end()
    }
}

struct RequestVisitor;

impl<'de> de::Visitor<'de> for RequestVisitor {
    type Value = Request;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("struct Request")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let payload_type: u8 =
            seq.next_element()?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
        match payload_type {
            TypeCodes::REVERSE => {
                let payload: Vec<u8> =
                    seq.next_element()?.ok_or_else(|| de::Error::invalid_length(1, &self))?;
                Ok(Request::Reverse(payload))
            }
            _ => Err(de::Error::invalid_value(
                de::Unexpected::Signed(payload_type.into()),
                &"valid request type",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for Request {
    fn deserialize<D>(deserializer: D) -> Result<Request, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(RequestVisitor)
    }
}

impl Serialize for Response {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(2))?;
        match self {
            Response::Reverse(arr) => {
                seq.serialize_element(&TypeCodes::REVERSE)?;
                seq.serialize_element(arr)?;
            }
        }
        seq.end()
    }
}

struct ResponseVisitor;

impl<'de> de::Visitor<'de> for ResponseVisitor {
    type Value = Response;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("struct Response")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let payload_type: u8 =
            seq.next_element()?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
        match payload_type {
            TypeCodes::REVERSE => {
                let payload: Vec<u8> =
                    seq.next_element()?.ok_or_else(|| de::Error::invalid_length(1, &self))?;
                Ok(Response::Reverse(payload))
            }
            _ => Err(de::Error::invalid_value(
                de::Unexpected::Signed(payload_type.into()),
                &"valid response type",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for Response {
    fn deserialize<D>(deserializer: D) -> Result<Response, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(ResponseVisitor)
    }
}
