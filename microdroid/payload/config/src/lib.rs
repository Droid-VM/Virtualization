// Copyright 2021, The Android Open Source Project
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

//! VM Payload Config

use serde::{Deserialize, Serialize};

/// VM Payload config
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VmPayloadConfig {
    /// Os config. Default: "microroid"
    #[serde(default = "OsConfig::default")]
    pub os: OsConfig,

    /// task to run
    #[serde(default)]
    pub task: Option<Task>,

    /// apexes to use
    #[serde(default)]
    pub apexes: Vec<ApexConfig>,
}

/// Os config
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OsConfig {
    /// The name of OS to use
    pub name: String,
}

impl Default for OsConfig {
    fn default() -> Self {
        Self { name: "microdroid".to_owned() }
    }
}

/// Payload's task can be one of plain executable
/// or an .so library which can be started via /system/bin/microdroid_launcher
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TaskType {
    /// Task's command indicates the path to the executable binary.
    #[serde(rename = "executable")]
    Executable,
    /// Task's command indicates the .so library in /mnt/apk/lib/{arch}
    #[serde(rename = "microdroid_launcher")]
    MicrodroidLauncher,
}

/// Task
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Task {
    /// Decides how to execute the command: executable(default) | microdroid_launcher
    #[serde(default = "default_task_type", rename = "type")]
    pub type_: TaskType,

    /// Command to run.
    /// - For executable task, this is the path to the executable.
    /// - For microdroid_launcher task, this is the name of .so
    pub command: String,

    /// Args to the command
    #[serde(default)]
    pub args: Vec<String>,
}

fn default_task_type() -> TaskType {
    TaskType::Executable
}

/// Apex config
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApexConfig {
    /// The name of APEX
    pub name: String,
}
