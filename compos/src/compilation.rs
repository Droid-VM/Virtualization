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

use anyhow::{anyhow, bail, Context, Result};
use log::{debug, error};
use minijail::{self, Minijail};
use std::default::Default;
use std::ffi::CStr;
use std::fs::File;
use std::iter::Iterator;
use std::ops::Drop;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;

use crate::fsverity;
use authfs_aidl_interface::aidl::com::android::virt::fs::{
    AuthFsConfig::AuthFsConfig, IAuthFs::IAuthFs, IAuthFsService::IAuthFsService,
    InputFdAnnotation::InputFdAnnotation, OutputFdAnnotation::OutputFdAnnotation,
};
use authfs_aidl_interface::binder::{ParcelFileDescriptor, Strong};
use compos_aidl_interface::aidl::com::android::compos::FdAnnotation::FdAnnotation;

/// The number that represents the file descriptor number expecting by the task. The number may be
/// meaningless in the current process.
pub type PseudoRawFd = i32;

pub enum CompilerOutput {
    /// Fs-verity digests of output files, if the compiler finishes successfully.
    Digests {
        oat: fsverity::Sha256Digest,
        vdex: fsverity::Sha256Digest,
        image: fsverity::Sha256Digest,
    },
    /// Exit code returned by the compiler, if not 0.
    ExitCode(i8),
}

struct CompilerOutputParcelFds {
    oat: ParcelFileDescriptor,
    vdex: ParcelFileDescriptor,
    image: ParcelFileDescriptor,
}

struct NullTerminatedPtrIter<T> {
    base: *const *const T,
    index: usize,
}

impl<T> Iterator for NullTerminatedPtrIter<T> {
    type Item = *const T;

    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: Caller must ensure the pointer array is valid and null-terminated.
        let current_ptr = unsafe { *self.base.add(self.index) };
        if current_ptr.is_null() {
            None
        } else {
            self.index += 1;
            Some(current_ptr)
        }
    }
}

impl<T> NullTerminatedPtrIter<T> {
    /// SAFETY: Caller is responsible to provide a pointer that points to a null-terminated pointer
    /// array.
    unsafe fn new(base: *const *const T) -> Self {
        NullTerminatedPtrIter { base, index: 0 }
    }
}

/// Runs the compiler with given flags with file descriptors described in `fd_annotation` retrieved
/// via `authfs_service`. Returns exit code of the compiler process.
pub fn compile_cmd(
    compiler_path: &Path,
    compiler_args: &[String],
    authfs_service: Strong<dyn IAuthFsService>,
    fd_annotation: &FdAnnotation,
) -> Result<CompilerOutput> {
    // Mount authfs (via authfs_service). The authfs instance unmounts once the `authfs` variable
    // is out of scope.
    let authfs_config = build_authfs_config(fd_annotation);
    let authfs = authfs_service.mount(&authfs_config)?;

    // The task expects to receive FD numbers that match its flags (e.g. --zip-fd=42) prepared
    // on the host side. Since the local FD opened from authfs (e.g. /authfs/42) may not match
    // the task's expectation, prepare a FD mapping and let minijail prepare the correct FD
    // setup.
    let fd_mapping =
        open_authfs_files_for_fd_mapping(&authfs, &authfs_config).context("Open on authfs")?;

    let args: Vec<_> = compiler_args.iter().map(|s| s.as_str()).collect();
    let jail = spawn_jailed_task(compiler_path, &args, fd_mapping).context("Spawn dex2oat")?;
    let jail_result = jail.wait();

    let parcel_fds = parse_compiler_args(&authfs, compiler_args)?;
    let oat_file: &File = parcel_fds.oat.as_ref();
    let vdex_file: &File = parcel_fds.vdex.as_ref();
    let image_file: &File = parcel_fds.image.as_ref();

    match jail_result {
        Ok(()) => Ok(CompilerOutput::Digests {
            oat: fsverity::measure(oat_file.as_raw_fd())?,
            vdex: fsverity::measure(vdex_file.as_raw_fd())?,
            image: fsverity::measure(image_file.as_raw_fd())?,
        }),
        Err(minijail::Error::ReturnCode(exit_code)) => {
            error!("dex2oat failed with exit code {}", exit_code);
            Ok(CompilerOutput::ExitCode(exit_code as i8))
        }
        Err(e) => {
            bail!("Unexpected minijail error: {}", e)
        }
    }
}

struct DexoptContext {
    context: *const dexopt_bindgen::ADexoptContext,
}

impl Drop for DexoptContext {
    fn drop(&mut self) {
        // SAFETY: Safe to destruct the context since no one can access the context anymore.
        unsafe {
            dexopt_bindgen::ADexopt_DeleteDexoptContext(self.context);
        }
    }
}

impl DexoptContext {
    fn new(marshaled: &[u8]) -> Result<Self> {
        // SAFETY: ADexopt_CreateAndValidateDexoptContext only reads the memory range and does not
        // try to manage the pointer.
        //
        // Also, the byte buffer `marshaled` comes from an untrusted world. We have to rely on
        // ADexopt_CreateAndValidateDexoptContext to handle the unmarshalling and check safely and
        // correctly, and only after so, returns a valid opaque context object.
        let context = unsafe {
            dexopt_bindgen::ADexopt_CreateAndValidateDexoptContext(
                marshaled.as_ptr(),
                marshaled.len(),
            )
        };
        if context.is_null() {
            bail!("Can't create dexopt context");
        }
        Ok(DexoptContext { context })
    }

    fn build_cmdline<'a>(&self, compiler_path: &'a str) -> Result<Vec<&'a str>> {
        // SAFETY: The context returned earlier is assumed to be legitimate. As a result, the
        // pointer is assumed to pointer to a C string array that ends with a nullptr.
        let cmdline_args_iter = unsafe {
            NullTerminatedPtrIter::new(dexopt_bindgen::ADexopt_GetCmdlineArguments(self.context))
        };
        let results: Result<Vec<_>, _> = cmdline_args_iter
            .map(|ptr| {
                // SAFETY: ptr should already point to a valid C string in the context.
                unsafe { CStr::from_ptr(ptr) }.to_str()
            })
            .collect();
        let mut args = results?;

        let mut compiler_args = Vec::new();
        compiler_args.reserve_exact(1 + args.len());
        compiler_args.push(compiler_path);
        compiler_args.append(&mut args);
        Ok(compiler_args)
    }
}

/// Runs the compiler with given flags with file descriptors described in `fd_annotation` retrieved
/// via `authfs_service`. Returns exit code of the compiler process.
pub fn compile(
    compiler_path: &Path,
    marshaled: &[u8],
    fd_annotation: &FdAnnotation,
    authfs_service: Strong<dyn IAuthFsService>,
) -> Result<CompilerOutput> {
    // Get ready to prepare the context encoded in the marshaled bytes.
    let dexopt_context = DexoptContext::new(marshaled)?;

    // Mount authfs (via authfs_service). The authfs instance unmounts once the `authfs` variable
    // is out of scope.
    let authfs_config = build_authfs_config(fd_annotation);
    let authfs = authfs_service.mount(&authfs_config)?;

    // The task expects to receive FD numbers that match its flags (e.g. --zip-fd=42) prepared
    // on the host side. Since the local FD opened from authfs (e.g. /authfs/42) may not match
    // the task's expectation, prepare a FD mapping and let minijail prepare the correct FD
    // setup.
    let fd_mapping =
        open_authfs_files_for_fd_mapping(&authfs, &authfs_config).context("Open on authfs")?;

    let compiler_args = dexopt_context.build_cmdline(compiler_path.to_str().unwrap())?;
    let jail =
        spawn_jailed_task(compiler_path, &compiler_args, fd_mapping).context("Spawn dex2oat")?;
    let jail_result = jail.wait();

    match jail_result {
        Ok(()) => {
            let _digest_results: Result<Vec<_>> = fd_annotation
                .output_fds
                .iter()
                .map(|fd| {
                    let file = authfs.openFile(*fd, /* writable */ false)?;
                    fsverity::measure(file.as_raw_fd())
                })
                .collect();
            // TODO(victorhsieh): Figure out a better way to return signatures. This is not trivial
            // because the information of which FDs are oat/vdex/image is not available (only ART's
            // code knows). We may need another API from ART to tell us the FDs to sign.
            // Separately, we also need to decide to write the signatures to output signature FDs,
            // return by bytes, add a new ART API, or let even ART handle the signing.
            Ok(CompilerOutput::Digests {
                oat: Default::default(),
                vdex: Default::default(),
                image: Default::default(),
            })
        }
        Err(minijail::Error::ReturnCode(exit_code)) => {
            error!("dex2oat failed with exit code {}", exit_code);
            Ok(CompilerOutput::ExitCode(exit_code as i8))
        }
        Err(e) => {
            bail!("Unexpected minijail error: {}", e)
        }
    }
}

fn parse_compiler_args(
    authfs: &Strong<dyn IAuthFs>,
    args: &[String],
) -> Result<CompilerOutputParcelFds> {
    const OAT_FD_PREFIX: &str = "--oat-fd=";
    const VDEX_FD_PREFIX: &str = "--output-vdex-fd=";
    const IMAGE_FD_PREFIX: &str = "--image-fd=";
    const APP_IMAGE_FD_PREFIX: &str = "--app-image-fd=";

    let mut oat = None;
    let mut vdex = None;
    let mut image = None;

    for arg in args {
        if let Some(value) = arg.strip_prefix(OAT_FD_PREFIX) {
            let fd = value.parse::<RawFd>().context("Invalid --oat-fd flag")?;
            debug_assert!(oat.is_none());
            oat = Some(authfs.openFile(fd, false)?);
        } else if let Some(value) = arg.strip_prefix(VDEX_FD_PREFIX) {
            let fd = value.parse::<RawFd>().context("Invalid --output-vdex-fd flag")?;
            debug_assert!(vdex.is_none());
            vdex = Some(authfs.openFile(fd, false)?);
        } else if let Some(value) = arg.strip_prefix(IMAGE_FD_PREFIX) {
            let fd = value.parse::<RawFd>().context("Invalid --image-fd flag")?;
            debug_assert!(image.is_none());
            image = Some(authfs.openFile(fd, false)?);
        } else if let Some(value) = arg.strip_prefix(APP_IMAGE_FD_PREFIX) {
            let fd = value.parse::<RawFd>().context("Invalid --app-image-fd flag")?;
            debug_assert!(image.is_none());
            image = Some(authfs.openFile(fd, false)?);
        }
    }

    Ok(CompilerOutputParcelFds {
        oat: oat.ok_or_else(|| anyhow!("Missing --oat-fd"))?,
        vdex: vdex.ok_or_else(|| anyhow!("Missing --vdex-fd"))?,
        image: image.ok_or_else(|| anyhow!("Missing --image-fd or --app-image-fd"))?,
    })
}

fn build_authfs_config(fd_annotation: &FdAnnotation) -> AuthFsConfig {
    AuthFsConfig {
        port: 3264, // TODO: support dynamic port
        inputFdAnnotations: fd_annotation
            .input_fds
            .iter()
            .map(|fd| InputFdAnnotation { fd: *fd })
            .collect(),
        outputFdAnnotations: fd_annotation
            .output_fds
            .iter()
            .map(|fd| OutputFdAnnotation { fd: *fd })
            .collect(),
    }
}

fn open_authfs_files_for_fd_mapping(
    authfs: &Strong<dyn IAuthFs>,
    config: &AuthFsConfig,
) -> Result<Vec<(ParcelFileDescriptor, PseudoRawFd)>> {
    let mut fd_mapping = Vec::new();

    let results: Result<Vec<_>> = config
        .inputFdAnnotations
        .iter()
        .map(|annotation| Ok((authfs.openFile(annotation.fd, false)?, annotation.fd)))
        .collect();
    fd_mapping.append(&mut results?);

    let results: Result<Vec<_>> = config
        .outputFdAnnotations
        .iter()
        .map(|annotation| Ok((authfs.openFile(annotation.fd, true)?, annotation.fd)))
        .collect();
    fd_mapping.append(&mut results?);

    Ok(fd_mapping)
}

fn spawn_jailed_task(
    executable: &Path,
    args: &[&str],
    fd_mapping: Vec<(ParcelFileDescriptor, PseudoRawFd)>,
) -> Result<Minijail> {
    debug!("Running compiler in minijail. Args: {:?}", args);
    // TODO(b/185175567): Run in a more restricted sandbox.
    let jail = Minijail::new()?;
    let preserve_fds: Vec<_> = fd_mapping.iter().map(|(f, id)| (f.as_raw_fd(), *id)).collect();
    let _pid = jail.run_remap(executable, preserve_fds.as_slice(), args)?;
    Ok(jail)
}
