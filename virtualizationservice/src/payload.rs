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

//! Payload disk image

use android_system_virtualizationservice::aidl::android::system::virtualizationservice::{
    DiskImage::DiskImage, Partition::Partition, VirtualMachineAppConfig::VirtualMachineAppConfig,
    VirtualMachineRawConfig::VirtualMachineRawConfig,
};
use android_system_virtualizationservice::binder::ParcelFileDescriptor;
use anyhow::{anyhow, bail, Context, Result};
use microdroid_metadata::{ApexPayload, ApkPayload, Metadata};
use microdroid_payload_config::ApexConfig;
use once_cell::sync::OnceCell;
use regex::Regex;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use vmconfig::open_parcel_file;

/// The list of APEXes which microdroid requires.
// TODO(b/192200378) move this to microdroid.json?
const MICRODROID_REQUIRED_APEXES: [&str; 3] =
    ["com.android.adbd", "com.android.i18n", "com.android.os.statsd"];

/// Represents the list of APEXes
#[derive(Debug)]
struct ApexInfoList {
    list: Vec<ApexInfo>,
}

#[derive(Debug)]
struct ApexInfo {
    name: String,
    path: PathBuf,
}

impl ApexInfoList {
    /// Loads ApexInfoList
    fn load() -> Result<&'static ApexInfoList> {
        static INSTANCE: OnceCell<ApexInfoList> = OnceCell::new();
        INSTANCE.get_or_try_init(|| {
            // TODO(b/191601801): look up /apex/apex-info-list.xml instead of apexservice
            // Each APEX prints the line:
            //   Module: <...> Version: <...> VersionName: <...> Path: <...> IsActive: <...> IsFactory: <...>
            // We only care about "Module:" and "Path:" tagged values for now.
            let info_pattern =
                Regex::new(r"^Module: (?P<name>[^ ]*) .* Path: (?P<path>[^ ]*) .*$")?;
            let output = Command::new("cmd")
                .arg("-w")
                .arg("apexservice")
                .arg("getActivePackages")
                .output()
                .expect("failed to execute apexservice cmd");
            let list = BufReader::new(output.stdout.as_slice())
                .lines()
                .map(|line| -> Result<ApexInfo> {
                    let line = line?;
                    let captures = info_pattern
                        .captures(&line)
                        .ok_or_else(|| anyhow!("can't parse: {}", line))?;
                    let name = captures.name("name").unwrap();
                    let path = captures.name("path").unwrap();
                    Ok(ApexInfo { name: name.as_str().to_owned(), path: path.as_str().into() })
                })
                .collect::<Result<Vec<ApexInfo>>>()?;
            if list.is_empty() {
                bail!("failed to load apex info: empty");
            }
            Ok(ApexInfoList { list })
        })
    }

    fn get_path_for(&self, apex_name: &str) -> Result<PathBuf> {
        Ok(self
            .list
            .iter()
            .find(|apex| apex.name == apex_name)
            .ok_or_else(|| anyhow!("{} not found.", apex_name))?
            .path
            .clone())
    }
}

fn make_metadata_file(
    config_path: &str,
    apexes: &[ApexConfig],
    temporary_directory: &Path,
) -> Result<ParcelFileDescriptor> {
    let metadata_path = temporary_directory.join("metadata");
    let metadata = Metadata {
        version: 1,
        apexes: apexes
            .iter()
            .map(|apex| ApexPayload { name: apex.name.clone(), ..Default::default() })
            .collect(),
        apk: Some(ApkPayload {
            name: "apk".to_owned(),
            payload_partition_name: "microdroid-apk".to_owned(),
            idsig_partition_name: "microdroid-apk-idsig".to_owned(),
            ..Default::default()
        })
        .into(),
        payload_config_path: format!("/mnt/apk/{}", config_path),
        ..Default::default()
    };

    // Write metadata to file.
    let mut metadata_file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&metadata_path)
        .with_context(|| format!("Failed to open metadata file {:?}", metadata_path))?;
    microdroid_metadata::write_metadata(&metadata, &mut metadata_file)?;

    // Re-open the metadata file as read-only.
    open_parcel_file(&metadata_path, false)
}

/// Creates a DiskImage with partitions:
///   metadata: metadata
///   microdroid-apex-0: apex 0
///   microdroid-apex-1: apex 1
///   ..
///   microdroid-apk: apk
///   microdroid-apk-idsig: idsig
fn make_payload_disk(
    apk_file: File,
    idsig_file: File,
    config_path: &str,
    apexes: &[ApexConfig],
    temporary_directory: &Path,
) -> Result<DiskImage> {
    let metadata_file = make_metadata_file(config_path, apexes, temporary_directory)?;
    // put metadata at the first partition
    let mut partitions = vec![Partition {
        label: "payload-metadata".to_owned(),
        images: vec![metadata_file],
        writable: false,
    }];

    let apex_info_list = ApexInfoList::load()?;
    for (i, apex) in apexes.iter().enumerate() {
        let apex_path = apex_info_list.get_path_for(&apex.name)?;
        let apex_file = open_parcel_file(&apex_path, false)?;
        partitions.push(Partition {
            label: format!("microdroid-apex-{}", i),
            images: vec![apex_file],
            writable: false,
        });
    }
    partitions.push(Partition {
        label: "microdroid-apk".to_owned(),
        images: vec![ParcelFileDescriptor::new(apk_file)],
        writable: false,
    });
    partitions.push(Partition {
        label: "microdroid-apk-idsig".to_owned(),
        images: vec![ParcelFileDescriptor::new(idsig_file)],
        writable: false,
    });

    Ok(DiskImage { image: None, partitions, writable: false })
}

pub fn add_microdroid_images(
    config: &VirtualMachineAppConfig,
    temporary_directory: &Path,
    apk_file: File,
    idsig_file: File,
    mut apexes: Vec<ApexConfig>,
    vm_config: &mut VirtualMachineRawConfig,
) -> Result<()> {
    apexes.extend(
        MICRODROID_REQUIRED_APEXES.iter().map(|name| ApexConfig { name: name.to_string() }),
    );
    apexes.dedup_by(|a, b| a.name == b.name);

    vm_config.disks.push(make_payload_disk(
        apk_file,
        idsig_file,
        &config.configPath,
        &apexes,
        temporary_directory,
    )?);

    if config.debug {
        vm_config.disks[1].partitions.push(Partition {
            label: "bootconfig".to_owned(),
            images: vec![open_parcel_file(
                Path::new("/apex/com.android.virt/etc/microdroid_bootconfig.debug"),
                false,
            )?],
            writable: false,
        });
    }

    Ok(())
}
