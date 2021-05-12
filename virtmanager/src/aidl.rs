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

//! Implementation of the AIDL interface of the Virt Manager.

use crate::composite::make_composite_image;
use crate::crosvm::{CrosvmConfig, DiskFile, VmInstance};
use crate::qcow2::make_qcow2_image;
use crate::{Cid, FIRST_GUEST_CID};
use android_system_virtmanager::aidl::android::system::virtmanager::IVirtManager::IVirtManager;
use android_system_virtmanager::aidl::android::system::virtmanager::IVirtualMachine::{
    BnVirtualMachine, IVirtualMachine,
};
use android_system_virtmanager::aidl::android::system::virtmanager::IVirtualMachineCallback::IVirtualMachineCallback;
use android_system_virtmanager::aidl::android::system::virtmanager::VirtualMachineConfig::VirtualMachineConfig;
use android_system_virtmanager::aidl::android::system::virtmanager::VirtualMachineDebugInfo::VirtualMachineDebugInfo;
use android_system_virtmanager::binder::{
    self, BinderFeatures, Interface, ParcelFileDescriptor, StatusCode, Strong, ThreadState,
};
use command_fds::FdMapping;
use log::{debug, error, warn};
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

pub const BINDER_SERVICE_IDENTIFIER: &str = "android.system.virtmanager";

// TODO(qwandor): Use PermissionController once it is available to Rust.
/// Only processes running with one of these UIDs are allowed to call debug methods.
const DEBUG_ALLOWED_UIDS: [u32; 2] = [0, 2000];

/// Implementation of `IVirtManager`, the entry point of the AIDL service.
#[derive(Debug, Default)]
pub struct VirtManager {
    state: Mutex<State>,
}

impl Interface for VirtManager {}

impl IVirtManager for VirtManager {
    /// Create and start a new VM with the given configuration, assigning it the next available CID.
    ///
    /// Returns a binder `IVirtualMachine` object referring to it, as a handle for the client.
    fn startVm(
        &self,
        config: &VirtualMachineConfig,
        log_fd: Option<&ParcelFileDescriptor>,
    ) -> binder::Result<Strong<dyn IVirtualMachine>> {
        let state = &mut *self.state.lock().unwrap();
        let log_fd = log_fd.map(clone_file).transpose()?;
        let requester_uid = ThreadState::get_calling_uid();
        let requester_sid = ThreadState::with_calling_sid(|sid| {
            if let Some(sid) = sid {
                match sid.to_str() {
                    Ok(sid) => Ok(sid.to_owned()),
                    Err(e) => {
                        error!("SID was not valid UTF-8: {:?}", e);
                        Err(StatusCode::BAD_VALUE)
                    }
                }
            } else {
                error!("Missing SID on startVm");
                Err(StatusCode::UNKNOWN_ERROR)
            }
        })?;
        let requester_debug_pid = ThreadState::get_calling_pid();

        // Files which are referred to from composite and QCOW2 images. These must be mapped to the
        // crosvm child process, and not closed before it is started.
        let mut indirect_files = vec![];
        // Temporary files which should be deleted once the VM finishes running.
        let mut temporary_files = vec![];

        // Assemble disk images if needed.
        let disks = config
            .disks
            .iter()
            .map(|disk| {
                let mut image = if !disk.partitions.is_empty() {
                    if disk.image.is_some() {
                        warn!("DiskImage {:?} contains both image and partitions.", disk);
                        return Err(StatusCode::BAD_VALUE);
                    }
                    // TODO(qwandor): Delete this file when the VM exits.
                    let composite_image_filename = state.make_composite_image_filename();
                    let (image, partition_files) = make_composite_image(
                        &disk.partitions,
                        &composite_image_filename,
                    )
                    .map_err(|e| {
                        error!("Failed to make composite image with config {:?}: {:?}", disk, e);
                        StatusCode::UNKNOWN_ERROR
                    })?;

                    indirect_files.extend(partition_files);
                    // Ensure that the composite image is deleted once the VM exits.
                    temporary_files.push(composite_image_filename);

                    image
                } else if let Some(image) = &disk.image {
                    clone_file(image)?
                } else {
                    warn!("DiskImage {:?} didn't contain image or partitions.", disk);
                    return Err(StatusCode::BAD_VALUE);
                };
                if disk.qcow2 {
                    let qcow2_image_filename = state.make_qcow2_image_filename();
                    let qcow2_image =
                        make_qcow2_image(&image, &qcow2_image_filename).map_err(|e| {
                            error!("Failed to make QCOW2 image: {:?}", e);
                            StatusCode::UNKNOWN_ERROR
                        })?;

                    // Ensure that the backing image isn't closed, and that its FD is passed to
                    // crosvm.
                    indirect_files.push(image);
                    // Ensure that the QCOW2 file is deleted once the VM exits.
                    temporary_files.push(qcow2_image_filename);

                    image = qcow2_image;
                }
                Ok(DiskFile { image, writable: disk.writable })
            })
            .collect::<Result<Vec<DiskFile>, StatusCode>>()?;

        let composite_disk_mappings: Vec<_> = indirect_files
            .iter()
            .map(|file| {
                let fd = file.as_raw_fd();
                FdMapping { parent_fd: fd, child_fd: fd }
            })
            .collect();

        // Actually start the VM.
        let cid = state.allocate_cid()?;
        let crosvm_config = CrosvmConfig {
            cid,
            bootloader: as_asref(&config.bootloader),
            kernel: as_asref(&config.kernel),
            initrd: as_asref(&config.initrd),
            disks,
            params: config.params.to_owned(),
        };
        let instance = VmInstance::start(
            &crosvm_config,
            log_fd,
            &composite_disk_mappings,
            temporary_files,
            requester_uid,
            requester_sid,
            requester_debug_pid,
        )
        .map_err(|e| {
            error!("Failed to start VM with config {:?}: {:?}", config, e);
            StatusCode::UNKNOWN_ERROR
        })?;
        state.add_vm(Arc::downgrade(&instance));
        Ok(VirtualMachine::create(instance))
    }

    /// Get a list of all currently running VMs. This method is only intended for debug purposes,
    /// and as such is only permitted from the shell user.
    fn debugListVms(&self) -> binder::Result<Vec<VirtualMachineDebugInfo>> {
        if !debug_access_allowed() {
            return Err(StatusCode::PERMISSION_DENIED.into());
        }

        let state = &mut *self.state.lock().unwrap();
        let vms = state.vms();
        let cids = vms
            .into_iter()
            .map(|vm| VirtualMachineDebugInfo {
                cid: vm.cid as i32,
                temporaryFiles: vm
                    .temporary_files
                    .iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect(),
                requesterUid: vm.requester_uid as i32,
                requesterSid: vm.requester_sid.clone(),
                requesterPid: vm.requester_debug_pid,
                running: vm.running(),
            })
            .collect();
        Ok(cids)
    }

    /// Hold a strong reference to a VM in Virt Manager. This method is only intended for debug
    /// purposes, and as such is only permitted from the shell user.
    fn debugHoldVmRef(&self, vmref: &Strong<dyn IVirtualMachine>) -> binder::Result<()> {
        if !debug_access_allowed() {
            return Err(StatusCode::PERMISSION_DENIED.into());
        }

        let state = &mut *self.state.lock().unwrap();
        state.debug_hold_vm(vmref.clone());
        Ok(())
    }

    /// Drop reference to a VM that is being held by Virt Manager. Returns the reference if VM was
    /// found and None otherwise. This method is only intended for debug purposes, and as such is
    /// only permitted from the shell user.
    fn debugDropVmRef(&self, cid: i32) -> binder::Result<Option<Strong<dyn IVirtualMachine>>> {
        if !debug_access_allowed() {
            return Err(StatusCode::PERMISSION_DENIED.into());
        }

        let state = &mut *self.state.lock().unwrap();
        Ok(state.debug_drop_vm(cid))
    }
}

/// Check whether the caller of the current Binder method is allowed to call debug methods.
fn debug_access_allowed() -> bool {
    let uid = ThreadState::get_calling_uid();
    log::trace!("Debug method call from UID {}.", uid);
    DEBUG_ALLOWED_UIDS.contains(&uid)
}

/// Implementation of the AIDL `IVirtualMachine` interface. Used as a handle to a VM.
#[derive(Debug)]
struct VirtualMachine {
    instance: Arc<VmInstance>,
}

impl VirtualMachine {
    fn create(instance: Arc<VmInstance>) -> Strong<dyn IVirtualMachine> {
        let binder = VirtualMachine { instance };
        BnVirtualMachine::new_binder(binder, BinderFeatures::default())
    }
}

impl Interface for VirtualMachine {}

impl IVirtualMachine for VirtualMachine {
    fn getCid(&self) -> binder::Result<i32> {
        Ok(self.instance.cid as i32)
    }

    fn isRunning(&self) -> binder::Result<bool> {
        Ok(self.instance.running())
    }

    fn registerCallback(
        &self,
        callback: &Strong<dyn IVirtualMachineCallback>,
    ) -> binder::Result<()> {
        // TODO: Should this give an error if the VM is already dead?
        self.instance.callbacks.add(callback.clone());
        Ok(())
    }
}

impl Drop for VirtualMachine {
    fn drop(&mut self) {
        debug!("Dropping {:?}", self);
        self.instance.kill();
    }
}

/// A set of Binders to be called back in response to various events on the VM, such as when it
/// dies.
#[derive(Debug, Default)]
pub struct VirtualMachineCallbacks(Mutex<Vec<Strong<dyn IVirtualMachineCallback>>>);

impl VirtualMachineCallbacks {
    /// Call all registered callbacks to say that the VM has died.
    pub fn callback_on_died(&self, cid: Cid) {
        let callbacks = &*self.0.lock().unwrap();
        for callback in callbacks {
            if let Err(e) = callback.onDied(cid as i32) {
                error!("Error calling callback: {}", e);
            }
        }
    }

    /// Add a new callback to the set.
    fn add(&self, callback: Strong<dyn IVirtualMachineCallback>) {
        self.0.lock().unwrap().push(callback);
    }
}

/// The mutable state of the Virt Manager. There should only be one instance of this struct.
#[derive(Debug)]
struct State {
    /// The next available unused CID.
    next_cid: Cid,

    /// The VMs which have been started. When VMs are started a weak reference is added to this list
    /// while a strong reference is returned to the caller over Binder. Once all copies of the
    /// Binder client are dropped the weak reference here will become invalid, and will be removed
    /// from the list opportunistically the next time `add_vm` is called.
    vms: Vec<Weak<VmInstance>>,

    /// Vector of strong VM references held on behalf of users that cannot hold them themselves.
    /// This is only used for debugging purposes.
    debug_held_vms: Vec<Strong<dyn IVirtualMachine>>,

    /// The unique number to use for the next composite image filename.
    next_composite_image_id: u64,
}

impl State {
    /// Get a list of VMs which still have Binder references to them.
    fn vms(&self) -> Vec<Arc<VmInstance>> {
        // Attempt to upgrade the weak pointers to strong pointers.
        self.vms.iter().filter_map(Weak::upgrade).collect()
    }

    /// Add a new VM to the list.
    fn add_vm(&mut self, vm: Weak<VmInstance>) {
        // Garbage collect any entries from the stored list which no longer exist.
        self.vms.retain(|vm| vm.strong_count() > 0);

        // Actually add the new VM.
        self.vms.push(vm);
    }

    /// Store a strong VM reference.
    fn debug_hold_vm(&mut self, vm: Strong<dyn IVirtualMachine>) {
        self.debug_held_vms.push(vm);
    }

    /// Retrieve and remove a strong VM reference.
    fn debug_drop_vm(&mut self, cid: i32) -> Option<Strong<dyn IVirtualMachine>> {
        let pos = self.debug_held_vms.iter().position(|vm| vm.getCid() == Ok(cid))?;
        Some(self.debug_held_vms.swap_remove(pos))
    }

    /// Get the next available CID, or an error if we have run out.
    fn allocate_cid(&mut self) -> binder::Result<Cid> {
        // TODO(qwandor): keep track of which CIDs are currently in use so that we can reuse them.
        let cid = self.next_cid;
        self.next_cid = self.next_cid.checked_add(1).ok_or(StatusCode::UNKNOWN_ERROR)?;
        Ok(cid)
    }

    /// Generates a unique filename to use for a QCOW2 disk image.
    fn make_qcow2_image_filename(&mut self) -> PathBuf {
        let id = self.next_composite_image_id;
        self.next_composite_image_id += 1;
        format!("/data/virtmanager/qcow2-{}.qcow2", id).into()
    }

    /// Generates a unique filename to use for a composite disk image.
    fn make_composite_image_filename(&mut self) -> PathBuf {
        let id = self.next_composite_image_id;
        self.next_composite_image_id += 1;
        format!("/data/virtmanager/composite-{}.img", id).into()
    }
}

impl Default for State {
    fn default() -> Self {
        State {
            next_cid: FIRST_GUEST_CID,
            vms: vec![],
            debug_held_vms: vec![],
            next_composite_image_id: 0,
        }
    }
}

/// Converts an `&Option<T>` to an `Option<U>` where `T` implements `AsRef<U>`.
fn as_asref<T: AsRef<U>, U>(option: &Option<T>) -> Option<&U> {
    option.as_ref().map(|t| t.as_ref())
}

/// Converts a `&ParcelFileDescriptor` to a `File` by cloning the file.
fn clone_file(file: &ParcelFileDescriptor) -> Result<File, StatusCode> {
    file.as_ref().try_clone().map_err(|_| StatusCode::UNKNOWN_ERROR)
}
