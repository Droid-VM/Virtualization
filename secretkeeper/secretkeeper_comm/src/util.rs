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
use crate::data_types::error::InternalError;
use ciborium::Value;

/// Decodes the provided binary CBOR-encoded value and returns a
/// ciborium::Value struct wrapped in Result.
pub fn value_from_bytes(mut bytes: &[u8]) -> Result<Value, InternalError> {
    let value =
        ciborium::de::from_reader(&mut bytes).map_err(|_| InternalError::ConversionError)?;
    // Ciborium tries to read one Value, but doesn't care if there is trailing data after it. We do
    if !bytes.is_empty() {
        return Err(InternalError::ConversionError);
    }
    Ok(value)
}

/// Encodes a ciborium::Value into bytes.
pub fn value_to_bytes(value: &Value) -> Result<Vec<u8>, InternalError> {
    let mut bytes: Vec<u8> = Vec::new();
    ciborium::ser::into_writer(&value, &mut bytes).map_err(|_| InternalError::UnexpectedError)?;
    Ok(bytes)
}

// Useful to convert cbor `Value` to integer, we return largest integer range for convenience,
// callers maybe downcast into appropriate type.
pub fn value_to_integer(value: &Value) -> Result<i128, InternalError> {
    let num = value.clone().into_integer().map_err(|_| InternalError::ConversionError)?.into();
    Ok(num)
}
