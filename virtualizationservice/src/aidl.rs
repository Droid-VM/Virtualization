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

//! Implementation of the AIDL interface of the VirtualizationService.

use crate::{get_calling_pid, get_calling_uid, REMOTELY_PROVISIONED_COMPONENT_SERVICE_NAME};
use crate::atom::{forward_vm_booted_atom, forward_vm_creation_atom, forward_vm_exited_atom};
use crate::rkpvm::request_attestation;
use android_os_permissions_aidl::aidl::android::os::IPermissionController;
use android_system_virtualizationcommon::aidl::android::system::virtualizationcommon::Certificate::Certificate;
use android_system_virtualizationservice::{
    aidl::android::system::virtualizationservice::AssignableDevice::AssignableDevice,
    aidl::android::system::virtualizationservice::VirtualMachineDebugInfo::VirtualMachineDebugInfo,
    binder::ParcelFileDescriptor,
};
use android_system_virtualizationservice_internal::aidl::android::system::virtualizationservice_internal::{
    AtomVmBooted::AtomVmBooted,
    AtomVmCreationRequested::AtomVmCreationRequested,
    AtomVmExited::AtomVmExited,
    IBoundDevice::IBoundDevice,
    IGlobalVmContext::{BnGlobalVmContext, IGlobalVmContext},
    IVirtualizationServiceInternal::IVirtualizationServiceInternal,
    IVfioHandler::{BpVfioHandler, IVfioHandler},
    IVfioHandler::VfioDev::VfioDev,
};
use android_system_virtualmachineservice::aidl::android::system::virtualmachineservice::IVirtualMachineService::VM_TOMBSTONES_SERVICE_PORT;
use anyhow::{anyhow, ensure, Context, Result};
use avflog::LogResult;
use binder::{self, wait_for_interface, BinderFeatures, ExceptionCode, Interface, LazyServiceGuard, Status, Strong, IntoBinderResult};
use lazy_static::lazy_static;
use libc::VMADDR_CID_HOST;
use log::{error, info, warn};
use rkpd_client::get_rkpd_attestation_key;
use rustutils::system_properties;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, create_dir, remove_dir_all, remove_file, set_permissions, File, Permissions};
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::raw::{pid_t, uid_t};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use tombstoned_client::{DebuggerdDumpType, TombstonedConnection};
use vsock::{VsockListener, VsockStream};
use nix::unistd::{chown, Uid};
use openssl::x509::X509;
use libfdt::Fdt;

// TODO(b/308694211): Use cstr! from vmbase instead.
macro_rules! cstr {
    ($str:literal) => {{
        const S: &str = concat!($str, "\0");
        const C: &::core::ffi::CStr = match ::core::ffi::CStr::from_bytes_with_nul(S.as_bytes()) {
            Ok(v) => v,
            Err(_) => panic!("string contains interior NUL"),
        };
        C
    }};
}

/// The unique ID of a VM used (together with a port number) for vsock communication.
pub type Cid = u32;

pub const BINDER_SERVICE_IDENTIFIER: &str = "android.system.virtualizationservice";

/// Directory in which to write disk image files used while running VMs.
pub const TEMPORARY_DIRECTORY: &str = "/data/misc/virtualizationservice";

/// The first CID to assign to a guest VM managed by the VirtualizationService. CIDs lower than this
/// are reserved for the host or other usage.
const GUEST_CID_MIN: Cid = 2048;
const GUEST_CID_MAX: Cid = 65535;

const SYSPROP_LAST_CID: &str = "virtualizationservice.state.last_cid";

const CHUNK_RECV_MAX_LEN: usize = 1024;

const VM_REFERENCE_DT_MAX_SIZE: usize = 1024;

lazy_static! {
    static ref VFIO_SERVICE: Strong<dyn IVfioHandler> =
        wait_for_interface(<BpVfioHandler as IVfioHandler>::get_descriptor())
            .expect("Could not connect to VfioHandler");
}

fn is_valid_guest_cid(cid: Cid) -> bool {
    (GUEST_CID_MIN..=GUEST_CID_MAX).contains(&cid)
}

/// Singleton service for allocating globally-unique VM resources, such as the CID, and running
/// singleton servers, like tombstone receiver.
#[derive(Debug, Default)]
pub struct VirtualizationServiceInternal {
    /// Cached read-only FD of VM reference DT file.
    reference_dt_file: Option<File>,

    state: Arc<Mutex<GlobalState>>,
}

impl VirtualizationServiceInternal {
    pub fn init() -> Result<VirtualizationServiceInternal> {
        let path = get_or_create_common_dir()?.join("reference.dtb");
        if path.exists() {
            // All temporary files are deleted when the service is started.
            // If the file exists, the file is likely corrupted.
            remove_file(&path).context("Failed to clone cached VM reference DT file descriptor")?;
        }

        let reference_dt_file = create_reference_dt_file(Path::new("/sys/firmware/fdt"), &path)?;
        let service = Self { reference_dt_file, ..Default::default() };

        std::thread::spawn(|| {
            if let Err(e) = handle_stream_connection_tombstoned() {
                warn!("Error receiving tombstone from guest or writing them. Error: {:?}", e);
            }
        });

        Ok(service)
    }
}

impl Interface for VirtualizationServiceInternal {}

impl IVirtualizationServiceInternal for VirtualizationServiceInternal {
    fn removeMemlockRlimit(&self) -> binder::Result<()> {
        let pid = get_calling_pid();
        let lim = libc::rlimit { rlim_cur: libc::RLIM_INFINITY, rlim_max: libc::RLIM_INFINITY };

        // SAFETY: borrowing the new limit struct only
        let ret = unsafe { libc::prlimit(pid, libc::RLIMIT_MEMLOCK, &lim, std::ptr::null_mut()) };

        match ret {
            0 => Ok(()),
            -1 => Err(std::io::Error::last_os_error().into()),
            n => Err(anyhow!("Unexpected return value from prlimit(): {n}")),
        }
        .or_binder_exception(ExceptionCode::ILLEGAL_STATE)
    }

    fn allocateGlobalVmContext(
        &self,
        requester_debug_pid: i32,
    ) -> binder::Result<Strong<dyn IGlobalVmContext>> {
        check_manage_access()?;

        let requester_uid = get_calling_uid();
        let requester_debug_pid = requester_debug_pid as pid_t;
        let state = &mut *self.state.lock().unwrap();
        state
            .allocate_vm_context(requester_uid, requester_debug_pid)
            .or_binder_exception(ExceptionCode::ILLEGAL_STATE)
    }

    fn atomVmBooted(&self, atom: &AtomVmBooted) -> Result<(), Status> {
        forward_vm_booted_atom(atom);
        Ok(())
    }

    fn atomVmCreationRequested(&self, atom: &AtomVmCreationRequested) -> Result<(), Status> {
        forward_vm_creation_atom(atom);
        Ok(())
    }

    fn atomVmExited(&self, atom: &AtomVmExited) -> Result<(), Status> {
        forward_vm_exited_atom(atom);
        Ok(())
    }

    fn debugListVms(&self) -> binder::Result<Vec<VirtualMachineDebugInfo>> {
        check_debug_access()?;

        let state = &mut *self.state.lock().unwrap();
        let cids = state
            .held_contexts
            .iter()
            .filter_map(|(_, inst)| Weak::upgrade(inst))
            .map(|vm| VirtualMachineDebugInfo {
                cid: vm.cid as i32,
                temporaryDirectory: vm.get_temp_dir().to_string_lossy().to_string(),
                requesterUid: vm.requester_uid as i32,
                requesterPid: vm.requester_debug_pid,
            })
            .collect();
        Ok(cids)
    }

    fn requestAttestation(
        &self,
        csr: &[u8],
        requester_uid: i32,
    ) -> binder::Result<Vec<Certificate>> {
        check_manage_access()?;
        if !cfg!(remote_attestation) {
            return Err(Status::new_exception_str(
                ExceptionCode::UNSUPPORTED_OPERATION,
                Some(
                    "requestAttestation is not supported with the remote_attestation feature \
                     disabled",
                ),
            ))
            .with_log();
        }
        info!("Received csr. Requestting attestation...");
        let attestation_key = get_rkpd_attestation_key(
            REMOTELY_PROVISIONED_COMPONENT_SERVICE_NAME,
            requester_uid as u32,
        )
        .context("Failed to retrieve the remotely provisioned keys")
        .with_log()
        .or_service_specific_exception(-1)?;
        let mut certificate_chain = split_x509_certificate_chain(&attestation_key.encodedCertChain)
            .context("Failed to split the remotely provisioned certificate chain")
            .with_log()
            .or_service_specific_exception(-1)?;
        if certificate_chain.is_empty() {
            return Err(Status::new_service_specific_error_str(
                -1,
                Some("The certificate chain should contain at least 1 certificate"),
            ))
            .with_log();
        }
        let certificate = request_attestation(
            csr.to_vec(),
            attestation_key.keyBlob,
            certificate_chain[0].encodedCertificate.clone(),
        )
        .context("Failed to request attestation")
        .with_log()
        .or_service_specific_exception(-1)?;
        certificate_chain.insert(0, Certificate { encodedCertificate: certificate });

        Ok(certificate_chain)
    }

    fn getAssignableDevices(&self) -> binder::Result<Vec<AssignableDevice>> {
        check_use_custom_virtual_machine()?;

        Ok(get_assignable_devices()?
            .device
            .into_iter()
            .map(|x| AssignableDevice { node: x.sysfs_path, kind: x.kind })
            .collect::<Vec<_>>())
    }

    fn bindDevicesToVfioDriver(
        &self,
        devices: &[String],
    ) -> binder::Result<Vec<Strong<dyn IBoundDevice>>> {
        check_use_custom_virtual_machine()?;

        let devices = get_assignable_devices()?
            .device
            .into_iter()
            .filter_map(|x| {
                if devices.contains(&x.sysfs_path) {
                    Some(VfioDev { sysfsPath: x.sysfs_path, dtboLabel: x.dtbo_label })
                } else {
                    warn!("device {} is not assignable", x.sysfs_path);
                    None
                }
            })
            .collect::<Vec<VfioDev>>();

        VFIO_SERVICE.bindDevicesToVfioDriver(devices.as_slice())
    }

    fn getDtboFile(&self) -> binder::Result<ParcelFileDescriptor> {
        check_use_custom_virtual_machine()?;

        let state = &mut *self.state.lock().unwrap();
        let file = state.get_dtbo_file().or_service_specific_exception(-1)?;
        Ok(ParcelFileDescriptor::new(file))
    }

    fn getReferenceDtFile(&self) -> binder::Result<Option<ParcelFileDescriptor>> {
        check_use_custom_virtual_machine()?;

        if let Some(file) = &self.reference_dt_file {
            let fd = file.try_clone().or_service_specific_exception(-1)?;
            Ok(Some(ParcelFileDescriptor::new(fd)))
        } else {
            Ok(None)
        }
    }
}

// KEEP IN SYNC WITH assignable_devices.xsd
#[derive(Debug, Deserialize)]
struct Device {
    kind: String,
    dtbo_label: String,
    sysfs_path: String,
}

#[derive(Debug, Default, Deserialize)]
struct Devices {
    device: Vec<Device>,
}

fn get_assignable_devices() -> binder::Result<Devices> {
    let xml_path = Path::new("/vendor/etc/avf/assignable_devices.xml");
    if !xml_path.exists() {
        return Ok(Devices { ..Default::default() });
    }

    let xml = fs::read(xml_path)
        .context("Failed to read assignable_devices.xml")
        .with_log()
        .or_service_specific_exception(-1)?;

    let xml = String::from_utf8(xml)
        .context("assignable_devices.xml is not a valid UTF-8 file")
        .with_log()
        .or_service_specific_exception(-1)?;

    let mut devices: Devices = serde_xml_rs::from_str(&xml)
        .context("can't parse assignable_devices.xml")
        .with_log()
        .or_service_specific_exception(-1)?;

    let mut device_set = HashSet::new();
    devices.device.retain(move |device| {
        if device_set.contains(&device.sysfs_path) {
            warn!("duplicated assignable device {device:?}; ignoring...");
            return false;
        }

        if !Path::new(&device.sysfs_path).exists() {
            warn!("assignable device {device:?} doesn't exist; ignoring...");
            return false;
        }

        device_set.insert(device.sysfs_path.clone());
        true
    });
    Ok(devices)
}

fn split_x509_certificate_chain(mut cert_chain: &[u8]) -> Result<Vec<Certificate>> {
    let mut out = Vec::new();
    while !cert_chain.is_empty() {
        let cert = X509::from_der(cert_chain)?;
        let end = cert.to_der()?.len();
        out.push(Certificate { encodedCertificate: cert_chain[..end].to_vec() });
        cert_chain = &cert_chain[end..];
    }
    Ok(out)
}

#[derive(Debug, Default)]
struct GlobalVmInstance {
    /// The unique CID assigned to the VM for vsock communication.
    cid: Cid,
    /// UID of the client who requested this VM instance.
    requester_uid: uid_t,
    /// PID of the client who requested this VM instance.
    requester_debug_pid: pid_t,
}

impl GlobalVmInstance {
    fn get_temp_dir(&self) -> PathBuf {
        let cid = self.cid;
        format!("{TEMPORARY_DIRECTORY}/{cid}").into()
    }
}

/// The mutable state of the VirtualizationServiceInternal. There should only be one instance
/// of this struct.
#[derive(Debug, Default)]
struct GlobalState {
    /// VM contexts currently allocated to running VMs. A CID is never recycled as long
    /// as there is a strong reference held by a GlobalVmContext.
    held_contexts: HashMap<Cid, Weak<GlobalVmInstance>>,

    /// Cached read-only FD of VM DTBO file. Also serves as a lock for creating the file.
    dtbo_file: Mutex<Option<File>>,
}

impl GlobalState {
    /// Get the next available CID, or an error if we have run out. The last CID used is stored in
    /// a system property so that restart of virtualizationservice doesn't reuse CID while the host
    /// Android is up.
    fn get_next_available_cid(&mut self) -> Result<Cid> {
        // Start trying to find a CID from the last used CID + 1. This ensures
        // that we do not eagerly recycle CIDs. It makes debugging easier but
        // also means that retrying to allocate a CID, eg. because it is
        // erroneously occupied by a process, will not recycle the same CID.
        let last_cid_prop =
            system_properties::read(SYSPROP_LAST_CID)?.and_then(|val| match val.parse::<Cid>() {
                Ok(num) => {
                    if is_valid_guest_cid(num) {
                        Some(num)
                    } else {
                        error!("Invalid value '{}' of property '{}'", num, SYSPROP_LAST_CID);
                        None
                    }
                }
                Err(_) => {
                    error!("Invalid value '{}' of property '{}'", val, SYSPROP_LAST_CID);
                    None
                }
            });

        let first_cid = if let Some(last_cid) = last_cid_prop {
            if last_cid == GUEST_CID_MAX {
                GUEST_CID_MIN
            } else {
                last_cid + 1
            }
        } else {
            GUEST_CID_MIN
        };

        let cid = self
            .find_available_cid(first_cid..=GUEST_CID_MAX)
            .or_else(|| self.find_available_cid(GUEST_CID_MIN..first_cid))
            .ok_or_else(|| anyhow!("Could not find an available CID."))?;

        system_properties::write(SYSPROP_LAST_CID, &format!("{}", cid))?;
        Ok(cid)
    }

    fn find_available_cid<I>(&self, mut range: I) -> Option<Cid>
    where
        I: Iterator<Item = Cid>,
    {
        range.find(|cid| !self.held_contexts.contains_key(cid))
    }

    fn allocate_vm_context(
        &mut self,
        requester_uid: uid_t,
        requester_debug_pid: pid_t,
    ) -> Result<Strong<dyn IGlobalVmContext>> {
        // Garbage collect unused VM contexts.
        self.held_contexts.retain(|_, instance| instance.strong_count() > 0);

        let cid = self.get_next_available_cid()?;
        let instance = Arc::new(GlobalVmInstance { cid, requester_uid, requester_debug_pid });
        create_temporary_directory(&instance.get_temp_dir(), Some(requester_uid))?;

        self.held_contexts.insert(cid, Arc::downgrade(&instance));
        let binder = GlobalVmContext { instance, ..Default::default() };
        Ok(BnGlobalVmContext::new_binder(binder, BinderFeatures::default()))
    }

    fn get_dtbo_file(&mut self) -> Result<File> {
        let mut file = self.dtbo_file.lock().unwrap();

        let fd = if let Some(ref_fd) = &*file {
            ref_fd.try_clone()?
        } else {
            let path = get_or_create_common_dir()?.join("vm.dtbo");
            if path.exists() {
                // All temporary files are deleted when the service is started.
                // If the file exists but the FD is not cached, the file is
                // likely corrupted.
                remove_file(&path).context("Failed to clone cached VM DTBO file descriptor")?;
            }

            // Open a write-only file descriptor for vfio_handler.
            let write_fd = File::create(&path).context("Failed to create VM DTBO file")?;
            VFIO_SERVICE.writeVmDtbo(&ParcelFileDescriptor::new(write_fd))?;

            // Open read-only. This FD will be cached and returned to clients.
            let read_fd = File::open(&path).context("Failed to open VM DTBO file")?;
            let read_fd_clone =
                read_fd.try_clone().context("Failed to clone VM DTBO file descriptor")?;
            *file = Some(read_fd);
            read_fd_clone
        };

        Ok(fd)
    }
}

fn create_temporary_directory(path: &PathBuf, requester_uid: Option<uid_t>) -> Result<()> {
    // Directory may exist if previous attempt to create it had failed.
    // Delete it before trying again.
    if path.as_path().exists() {
        remove_temporary_dir(path).unwrap_or_else(|e| {
            warn!("Could not delete temporary directory {:?}: {}", path, e);
        });
    }
    // Create directory.
    create_dir(path).with_context(|| format!("Could not create temporary directory {:?}", path))?;
    // If provided, change ownership to client's UID but system's GID, and permissions 0700.
    // If the chown() fails, this will leave behind an empty directory that will get removed
    // at the next attempt, or if virtualizationservice is restarted.
    if let Some(uid) = requester_uid {
        chown(path, Some(Uid::from_raw(uid)), None).with_context(|| {
            format!("Could not set ownership of temporary directory {:?}", path)
        })?;
    }
    Ok(())
}

/// Removes a directory owned by a different user by first changing its owner back
/// to VirtualizationService.
pub fn remove_temporary_dir(path: &PathBuf) -> Result<()> {
    ensure!(path.as_path().is_dir(), "Path {:?} is not a directory", path);
    chown(path, Some(Uid::current()), None)?;
    set_permissions(path, Permissions::from_mode(0o700))?;
    remove_dir_all(path)?;
    Ok(())
}

fn get_or_create_common_dir() -> Result<PathBuf> {
    let path = Path::new(TEMPORARY_DIRECTORY).join("common");
    if !path.exists() {
        create_temporary_directory(&path, None)?;
    }
    Ok(path)
}

fn create_reference_dt_file(fdt_path: &Path, reference_dt_path: &Path) -> Result<Option<File>> {
    // VTS checks /sys/firmware/fdt, but CF doesn't have this.
    let src_data = match fs::read(fdt_path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!("{fdt_path:?} doesn't exist");
            println!("{fdt_path:?} doesn't exist");
            return Ok(None);
        }
        e => e.context("Failed to read {fdt_path:?}")?,
    };

    let src_fdt =
        Fdt::from_slice(&src_data).map_err(|e| anyhow!("Failed to parse host DT file, {e:?}"))?;

    let Some(src_node) = src_fdt
        .node(cstr!("/avf/reference"))
        .map_err(|e| anyhow!("Failed to read host DT, {e:?}"))?
    else {
        warn!("Failed to find VM reference DT node from host DT");
        println!("Failed to find VM reference DT node from host DT");
        return Ok(None);
    };

    let mut dst_data = vec![0_u8; VM_REFERENCE_DT_MAX_SIZE];
    let dst_fdt = Fdt::create_empty_tree(dst_data.as_mut_slice())
        .map_err(|e| anyhow!("Failed to create VM reference DT, {e:?}"))?;

    let mut stack = vec![(CString::from(cstr!("/")), src_node)];

    while let Some((dst_path, src_node)) = stack.pop() {
        let mut dst_node = dst_fdt
            .node_mut(&dst_path)
            .map_err(|e| anyhow!("Failed to write VM reference DT, {e:?}"))?
            .ok_or_else(|| anyhow!("Internal error when writing VM reference DT"))?;

        let subnodes = src_node.subnodes().map_err(|e| anyhow!("Failed to read host DT, {e:?}"))?;
        for src_subnode in subnodes {
            let subnode_name =
                src_subnode.name().map_err(|e| anyhow!("Failed to read host DT, {e:?}"))?;
            dst_node
                .add_subnode(subnode_name)
                .map_err(|e| anyhow!("Failed to copy node from host DT, {e:?}"))?;

            // Note: keep path of new nodes instead because of borrow checker.
            let new_path = CString::from_vec_with_nul(
                [dst_path.to_bytes(), b"/", subnode_name.to_bytes(), b"\0"].concat(),
            )?;
            stack.push((new_path, src_subnode));
        }

        let properties = src_node
            .properties()
            .map_err(|e| anyhow!("Failed to get host DT properties, {e:?}"))?;
        for prop in properties {
            let name = prop
                .name()
                .map_err(|e| anyhow!("Failed to get property name from host DT, {e:?}"))?;
            let value = prop
                .value()
                .map_err(|e| anyhow!("Failed to get property value from host DT, {e:?}"))?;
            dst_node.setprop(name, value).map_err(|e| anyhow!("Failed to copy property, {e:?}"))?;
        }
    }

    fs::write(reference_dt_path, dst_fdt.as_slice())
        .context("Failed to create VM referencet DT file")?;

    // Open read-only. No reason to write.
    Ok(Some(File::open(reference_dt_path).context("Failed to open VM reference DT file")?))
}

/// Implementation of the AIDL `IGlobalVmContext` interface.
#[derive(Debug, Default)]
struct GlobalVmContext {
    /// Strong reference to the context's instance data structure.
    instance: Arc<GlobalVmInstance>,
    /// Keeps our service process running as long as this VM context exists.
    #[allow(dead_code)]
    lazy_service_guard: LazyServiceGuard,
}

impl Interface for GlobalVmContext {}

impl IGlobalVmContext for GlobalVmContext {
    fn getCid(&self) -> binder::Result<i32> {
        Ok(self.instance.cid as i32)
    }

    fn getTemporaryDirectory(&self) -> binder::Result<String> {
        Ok(self.instance.get_temp_dir().to_string_lossy().to_string())
    }
}

fn handle_stream_connection_tombstoned() -> Result<()> {
    // Should not listen for tombstones on a guest VM's port.
    assert!(!is_valid_guest_cid(VM_TOMBSTONES_SERVICE_PORT as Cid));
    let listener =
        VsockListener::bind_with_cid_port(VMADDR_CID_HOST, VM_TOMBSTONES_SERVICE_PORT as Cid)?;
    for incoming_stream in listener.incoming() {
        let mut incoming_stream = match incoming_stream {
            Err(e) => {
                warn!("invalid incoming connection: {:?}", e);
                continue;
            }
            Ok(s) => s,
        };
        std::thread::spawn(move || {
            if let Err(e) = handle_tombstone(&mut incoming_stream) {
                error!("Failed to write tombstone- {:?}", e);
            }
        });
    }
    Ok(())
}

fn handle_tombstone(stream: &mut VsockStream) -> Result<()> {
    if let Ok(addr) = stream.peer_addr() {
        info!("Vsock Stream connected to cid={} for tombstones", addr.cid());
    }
    let tb_connection =
        TombstonedConnection::connect(std::process::id() as i32, DebuggerdDumpType::Tombstone)
            .context("Failed to connect to tombstoned")?;
    let mut text_output = tb_connection
        .text_output
        .as_ref()
        .ok_or_else(|| anyhow!("Could not get file to write the tombstones on"))?;
    let mut num_bytes_read = 0;
    loop {
        let mut chunk_recv = [0; CHUNK_RECV_MAX_LEN];
        let n = stream
            .read(&mut chunk_recv)
            .context("Failed to read tombstone data from Vsock stream")?;
        if n == 0 {
            break;
        }
        num_bytes_read += n;
        text_output.write_all(&chunk_recv[0..n]).context("Failed to write guests tombstones")?;
    }
    info!("Received {} bytes from guest & wrote to tombstone file", num_bytes_read);
    tb_connection.notify_completion()?;
    Ok(())
}

/// Checks whether the caller has a specific permission
fn check_permission(perm: &str) -> binder::Result<()> {
    let calling_pid = get_calling_pid();
    let calling_uid = get_calling_uid();
    // Root can do anything
    if calling_uid == 0 {
        return Ok(());
    }
    let perm_svc: Strong<dyn IPermissionController::IPermissionController> =
        binder::get_interface("permission")?;
    if perm_svc.checkPermission(perm, calling_pid, calling_uid as i32)? {
        Ok(())
    } else {
        Err(anyhow!("does not have the {} permission", perm))
            .or_binder_exception(ExceptionCode::SECURITY)
    }
}

/// Check whether the caller of the current Binder method is allowed to call debug methods.
fn check_debug_access() -> binder::Result<()> {
    check_permission("android.permission.DEBUG_VIRTUAL_MACHINE")
}

/// Check whether the caller of the current Binder method is allowed to manage VMs
fn check_manage_access() -> binder::Result<()> {
    check_permission("android.permission.MANAGE_VIRTUAL_MACHINE")
}

/// Check whether the caller of the current Binder method is allowed to use custom VMs
fn check_use_custom_virtual_machine() -> binder::Result<()> {
    check_permission("android.permission.USE_CUSTOM_VIRTUAL_MACHINE")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::fs;

    const TEST_RKP_CERT_CHAIN_PATH: &str = "testdata/rkp_cert_chain.der";
    const TEST_VM_REFERENCE_DT_PATH: &str = "test_vm_reference_dt.dtb";
    const TEST_VM_REFERENCE_DT_BASE_PATH: &str = "test_vm_reference_dt_base.dtb";

    #[test]
    fn splitting_x509_certificate_chain_succeeds() -> Result<()> {
        let bytes = fs::read(TEST_RKP_CERT_CHAIN_PATH)?;
        let cert_chain = split_x509_certificate_chain(&bytes)?;

        assert_eq!(4, cert_chain.len());
        for cert in cert_chain {
            let x509_cert = X509::from_der(&cert.encodedCertificate)?;
            assert_eq!(x509_cert.to_der()?.len(), cert.encodedCertificate.len());
        }
        Ok(())
    }

    fn into_vec(numb: u32) -> Vec<u8> {
        numb.to_be_bytes().to_vec()
    }

    #[test]
    fn test_create_reference_dt_file() -> Result<()> {
        const TEST_FILE_NAME: &str = "test.dtb";
        let fdt_file = create_reference_dt_file(
            Path::new(TEST_VM_REFERENCE_DT_PATH),
            Path::new(TEST_FILE_NAME),
        )?;
        assert!(fdt_file.is_some());

        let mut fdt_data = Vec::new();
        fdt_file.unwrap().read_to_end(&mut fdt_data)?;

        let fdt_data_from_path = fs::read(TEST_FILE_NAME)?;
        assert_eq!(fdt_data, fdt_data_from_path);

        let fdt = Fdt::from_slice(&fdt_data)
            .map_err(|e| anyhow!("Failed to create a valid FDT, {e:?}"))?;

        let root = fdt.root().unwrap();
        let mut fdt_all_nodes: Vec<_> = root.descendants().collect();
        fdt_all_nodes.insert(0, (root, 0));

        let expected: Vec<(usize, &CStr, HashMap<&CStr, Vec<u8>>)> = vec![
            (
                0,
                cstr!(""),
                HashMap::from([
                    (
                        cstr!("vendor_hashtree_descriptor_root_digest"),
                        Vec::from(*b"this_is_test\0"),
                    ),
                    (cstr!("vendor_image_key"), Vec::from(*b"this_is_not_real\0")),
                ]),
            ),
            (1, cstr!("vendor"), HashMap::from([(cstr!("extra_prop"), into_vec(0xF_u32))])),
            (2, cstr!("vendor_extra_node"), HashMap::from([(cstr!("flag"), into_vec(0x9_u32))])),
            (1, cstr!("oem"), HashMap::from([(cstr!("stub"), into_vec(0x1_u32))])),
        ];

        assert_eq!(expected.len(), fdt_all_nodes.len());

        for ((expected_depth, expected_name, expected_props), (node, depth)) in
            expected.into_iter().zip(fdt_all_nodes)
        {
            println!("iterating {:?}", node.name());
            assert_eq!((expected_depth, Ok(expected_name)), (depth, node.name()));

            let props = HashMap::from_iter(
                node.properties()
                    .unwrap()
                    .map(|prop| (prop.name().unwrap(), prop.value().unwrap().to_vec())),
            );
            assert_eq!(expected_props, props);
        }

        Ok(())
    }

    #[test]
    fn test_create_reference_dt_from_empty_reference() -> Result<()> {
        let fdt_file = create_reference_dt_file(
            Path::new(TEST_VM_REFERENCE_DT_BASE_PATH),
            Path::new("test.dtb"),
        )?;
        assert!(fdt_file.is_none());
        Ok(())
    }
}
