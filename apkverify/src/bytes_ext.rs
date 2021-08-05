/*
 * Copyright (C) 2021 The Android Open Source Project
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

//! Provides extension methods Bytes::read<T>()

use anyhow::{bail, Result};
use bytes::{Buf, Bytes};
use std::ops::Deref;

#[derive(Clone)]
pub struct LengthPrefixed<T> {
    inner: T,
}

impl<T> Deref for LengthPrefixed<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub trait BytesExt: Buf {
    fn read<T: ReadFrom>(&mut self) -> Result<T>;
}

impl BytesExt for Bytes {
    fn read<T: ReadFrom>(&mut self) -> Result<T> {
        T::read_from(self)
    }
}

pub trait ReadFrom {
    fn read_from(buf: &mut Bytes) -> Result<Self>
    where
        Self: Sized;
}

impl ReadFrom for u32 {
    fn read_from(buf: &mut Bytes) -> Result<Self> {
        Ok(buf.get_u32_le())
    }
}

impl<T: ReadFrom> ReadFrom for Vec<T> {
    fn read_from(buf: &mut Bytes) -> Result<Self> {
        let mut result = vec![];
        while buf.has_remaining() {
            result.push(buf.read()?);
        }
        Ok(result)
    }
}

impl<T: ReadFrom> ReadFrom for LengthPrefixed<T> {
    fn read_from(buf: &mut Bytes) -> Result<Self> {
        let mut inner = read_length_prefixed_slice(buf)?;
        let inner = inner.read()?;
        Ok(LengthPrefixed { inner })
    }
}

impl ReadFrom for Bytes {
    fn read_from(buf: &mut Bytes) -> Result<Self> {
        Ok(buf.slice(..))
    }
}

fn read_length_prefixed_slice(buf: &mut Bytes) -> Result<Bytes> {
    if buf.remaining() < 4 {
        bail!(
            "Remaining buffer too short to contain length of length-prefixed field. Remaining: {}",
            buf.remaining()
        );
    }
    let len = buf.get_u32_le() as usize;
    if len > buf.remaining() {
        bail!(
            "length-prefixed field longer than remaining buffer. Field length: {}, remaining: {}",
            len,
            buf.remaining()
        );
    }
    Ok(buf.split_to(len))
}
