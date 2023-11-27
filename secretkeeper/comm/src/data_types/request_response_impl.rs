/*
 * Copyright (C) 2023 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Implementation of request & response like data structures.

// derive(N) generates a method that is missing a docstring.
#![allow(missing_docs)]

use crate::cbor_convert::value_to_integer;
use crate::data_types::cbor_ser::ValueConversion;
use crate::data_types::error::Error;
use crate::data_types::error::ERROR_OK;
use crate::data_types::request::Request;
use crate::data_types::response::Response;
use crate::data_types::{Id, Secret};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use ciborium::Value;
use enumn::N;

/// Set of all possible `Opcode` supported by SecretManagement API of the HAL.
/// See `Opcode` in SecretManagement.cddl
#[derive(Clone, Copy, Debug, N, PartialEq)]
#[non_exhaustive]
pub enum Opcode {
    /// Get version of the SecretManagement API.
    GetVersion = 1,
    /// Store a secret
    StoreSecret = 2,
    /// Get the secret
    GetSecret = 3,
}

/// Corresponds to `GetVersionRequestPacket` defined in SecretManagement.cddl
#[derive(Clone, Eq, PartialEq)]
pub struct GetVersionRequest;

impl Request for GetVersionRequest {
    const OPCODE: Opcode = Opcode::GetVersion;

    fn new(args: Vec<Value>) -> Result<Box<Self>, Error> {
        if !args.is_empty() {
            return Err(Error::RequestMalformed);
        }
        Ok(Box::new(Self))
    }

    fn args(&self) -> Vec<Value> {
        Vec::new()
    }
}

/// Success response corresponding to `GetVersionResponsePacket`.
#[derive(Clone, Eq, PartialEq)]
pub struct GetVersionResponse {
    /// Version of SecretManagement API
    version: u64,
}

impl GetVersionResponse {
    pub fn new(version: u64) -> Self {
        Self { version }
    }
    pub fn version(&self) -> u64 {
        self.version
    }
}

impl Response for GetVersionResponse {
    fn new(res: Vec<Value>) -> Result<Box<Self>, Error> {
        if res.len() != 2 {
            return Err(Error::ResponseMalformed);
        }
        let error_code: u16 = value_to_integer(&res[0])?.try_into()?;
        if error_code != ERROR_OK {
            return Err(Error::ResponseMalformed);
        }
        let version: u64 = value_to_integer(&res[1])?.try_into()?;
        Ok(Box::new(Self { version }))
    }

    fn result(&self) -> Vec<Value> {
        vec![self.version.into()]
    }
}

/// Corresponds to `StoreSecretRequestPacket` in SecretManagement.cddl
#[derive(Clone, Eq, PartialEq)]
pub struct StoreSecretRequest {
    // Unique identifier of the secret
    id: Id,
    // The secret to be stored :)
    secret: Secret,
    // The dice policy corresponding to the secret
    sealing_policy: Vec<u8>,
}

impl StoreSecretRequest {
    pub fn new(id: Id, secret: Secret, sealing_policy: Vec<u8>) -> Self {
        Self { id, secret, sealing_policy }
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn secret(&self) -> &Secret {
        &self.secret
    }

    pub fn sealing_policy(&self) -> &Vec<u8> {
        &self.sealing_policy
    }
}

impl Request for StoreSecretRequest {
    const OPCODE: Opcode = Opcode::StoreSecret;

    fn new(mut args: Vec<Value>) -> Result<Box<Self>, Error> {
        if args.len() != 3 {
            return Err(Error::RequestMalformed);
        }
        // TODO: Now that we have Box of array. Can we not pop!
        // We are using Vec::pop() to move elements out of vector (in reverse order) to save few
        // unnecessary clones.
        let sealing_policy = args.pop().expect("Vec empty, this is unexpected").into_bytes()?;
        let secret = args.pop().expect("Vec empty, this is unexpected");
        let secret = Secret::from_cbor_value(secret).map_err(|_| Error::RequestMalformed)?;
        let id = args.pop().expect("Vec empty, this is unexpected");
        let id: Id = Id::from_cbor_value(id).map_err(|_| Error::RequestMalformed)?;
        Ok(Box::new(Self { id, secret, sealing_policy }))
    }

    fn args(&self) -> Vec<Value> {
        vec![
            Value::from(self.id.0.as_slice()),
            Value::from(self.secret.0.as_slice()),
            Value::from(self.sealing_policy.clone()),
        ]
    }
}

/// Success response corresponding to `StoreSecretResponsePacket`.
#[derive(Clone, Eq, PartialEq)]
pub struct StoreSecretResponse {}

impl Response for StoreSecretResponse {
    fn new(response_cbor: Vec<Value>) -> Result<Box<Self>, Error> {
        if response_cbor.len() != 1 {
            return Err(Error::ResponseMalformed);
        }
        let error_code: u16 = value_to_integer(&response_cbor[0])?.try_into()?;
        if error_code != ERROR_OK {
            return Err(Error::ResponseMalformed);
        }
        Ok(Box::new(Self {}))
    }
}

/// Corresponds to `GetSecretRequestPacket` in SecretManagement.cddl
#[derive(Clone, Eq, PartialEq)]
pub struct GetSecretRequest {
    // Unique identifier of the secret.
    id: Id,
    // The updated dice_policy corresponding to the secret.
    updated_sealing_policy: Option<Vec<u8>>,
}

impl GetSecretRequest {
    pub fn new(id: Id, updated_sealing_policy: Option<Vec<u8>>) -> Self {
        Self { id, updated_sealing_policy }
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn updated_sealing_policy(&self) -> &Option<Vec<u8>> {
        &self.updated_sealing_policy
    }
}

impl Request for GetSecretRequest {
    const OPCODE: Opcode = Opcode::GetSecret;

    fn new(mut args: Vec<Value>) -> Result<Box<Self>, Error> {
        if args.len() != 2 {
            return Err(Error::RequestMalformed);
        }
        let sealing_policy_opt = args.pop().expect("Vec empty, this is unexpected");
        let updated_sealing_policy = if sealing_policy_opt.is_null() {
            None
        } else {
            Some(sealing_policy_opt.into_bytes()?)
        };
        let id = args.pop().expect("Vec empty, this is unexpected");
        let id: Id = Id::from_cbor_value(id).map_err(|_| Error::RequestMalformed)?;
        Ok(Box::new(Self { id, updated_sealing_policy }))
    }

    fn args(&self) -> Vec<Value> {
        let mut res = vec![Value::from(self.id.0.as_slice())];
        if let Some(policy) = &self.updated_sealing_policy {
            res.push(Value::from(policy.clone()));
        } else {
            res.push(Value::Null)
        }
        res
    }
}

/// Success response corresponding to `GetSecretResponsePacket`.
#[derive(Clone, Eq, PartialEq)]
pub struct GetSecretResponse {
    secret: Secret,
}

impl GetSecretResponse {
    pub fn new(secret: Secret) -> Self {
        Self { secret }
    }
    pub fn secret(self) -> Secret {
        self.secret
    }
}

impl Response for GetSecretResponse {
    fn new(mut res: Vec<Value>) -> Result<Box<Self>, Error> {
        if res.len() != 2 {
            return Err(Error::ResponseMalformed);
        }
        let secret = res.pop().expect("Vec empty, this is unexpected");
        let secret = Secret::from_cbor_value(secret).map_err(|_| Error::ResponseMalformed)?;
        let error_code: u16 =
            value_to_integer(&res.pop().expect("Vec empty, this is unexpected"))?.try_into()?;
        if error_code != ERROR_OK {
            return Err(Error::ResponseMalformed);
        }
        Ok(Box::new(Self { secret }))
    }

    fn result(&self) -> Vec<Value> {
        vec![self.secret.0.as_slice().into()]
    }
}
