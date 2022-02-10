/*
 * Copyright 2022 The Android Open Source Project
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

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

const COMPOS_KEY_HELPER_PATH: &str = "/apex/com.android.compos/bin/compos_key_helper";

pub fn get_public_key() -> Result<Vec<u8>> {
    let child = Command::new(COMPOS_KEY_HELPER_PATH)
        .arg("public_key")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = child.wait_with_output()?;
    if !result.status.success() {
        bail!("Helper failed: {:?}", result);
    }
    Ok(result.stdout)
}

pub fn sign(data: &[u8]) -> Result<Vec<u8>> {
    let mut child = Command::new(COMPOS_KEY_HELPER_PATH)
        .arg("sign")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // No output is written until the entire input is consumed, so this shouldn't deadlock.
    let wrote =
        child.stdin.take().unwrap().write_all(data).context("Failed to write data to be signed");
    if wrote.is_err() {
        // Just in case the error isn't due to the process having died.
        let _ignored = child.kill();
    }

    let result = child.wait_with_output()?;
    if !result.status.success() {
        bail!("Helper failed: {:?}", result);
    }
    wrote.map(|_| result.stdout)
}
