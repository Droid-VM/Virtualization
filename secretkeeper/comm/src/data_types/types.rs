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

use crate::data_types::error::Error;
use alloc::vec::Vec;

const ID_SIZE: usize = 64;
const SECRET_SIZE: usize = 32;

/// Identifier of Secret. See `id` in SecretManagement.cddl
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Id([u8; ID_SIZE]);
impl Id {
    /// Extract the inner bytes
    pub fn into_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for Id {
    type Error = crate::data_types::error::Error;
    fn try_from(vec: Vec<u8>) -> Result<Self, Error> {
        let arr: [u8; ID_SIZE] = vec.try_into().map_err(|_| Error::ConversionError)?;
        Ok(Self(arr))
    }
}

impl From<[u8; ID_SIZE]> for Id {
    fn from(arr: [u8; ID_SIZE]) -> Self {
        Self(arr)
    }
}

/// Data structure for Secret. Corresponds to `secret` in SecretManagement.cddl
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Secret([u8; SECRET_SIZE]);

impl Secret {
    /// Extract the inner bytes
    pub fn into_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for Secret {
    type Error = crate::data_types::error::Error;
    fn try_from(vec: Vec<u8>) -> Result<Self, Error> {
        let arr: [u8; SECRET_SIZE] = vec.try_into().map_err(|_| Error::ConversionError)?;
        Ok(Self(arr))
    }
}

impl From<[u8; SECRET_SIZE]> for Secret {
    fn from(arr: [u8; SECRET_SIZE]) -> Self {
        Self(arr)
    }
}
