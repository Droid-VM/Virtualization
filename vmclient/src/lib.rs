// Copyright 2022, The Android Open Source Project
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

//! Client library for VirtualizationService.

mod errors;
mod sync;

pub use crate::errors::VmWaitError;
use crate::sync::Monitor;
use android_system_virtualizationservice::{
    aidl::android::system::virtualizationservice::{
        DeathReason::DeathReason as AidlDeathReason,
        IVirtualMachine::IVirtualMachine,
        IVirtualMachineCallback::{BnVirtualMachineCallback, IVirtualMachineCallback},
        IVirtualizationService::IVirtualizationService,
        VirtualMachineConfig::VirtualMachineConfig,
        VirtualMachineRawConfig::VirtualMachineRawConfig,
        VirtualMachineState::VirtualMachineState,
    },
    binder::{
        wait_for_interface, BinderFeatures, DeathRecipient, IBinder, Interface,
        ParcelFileDescriptor, Result as BinderResult, StatusCode, Strong,
    },
};
use log::warn;
use std::{
    fmt::{self, Debug, Display, Formatter},
    fs::File,
    sync::Arc,
    time::Duration,
};

const VIRTUALIZATION_SERVICE_BINDER_SERVICE_IDENTIFIER: &str =
    "android.system.virtualizationservice";

/// Connects to the VirtualizationService AIDL service.
pub fn connect() -> Result<Strong<dyn IVirtualizationService>, StatusCode> {
    wait_for_interface(VIRTUALIZATION_SERVICE_BINDER_SERVICE_IDENTIFIER)
}

/// Starts a new VM with a raw configuration.
pub fn start_raw(
    service: &dyn IVirtualizationService,
    config: VirtualMachineRawConfig,
    console: Option<File>,
    log: Option<File>,
) -> BinderResult<VmInstance> {
    start(service, &VirtualMachineConfig::RawConfig(config), console, log)
}

fn start(
    service: &dyn IVirtualizationService,
    config: &VirtualMachineConfig,
    console: Option<File>,
    log: Option<File>,
) -> BinderResult<VmInstance> {
    let console = console.map(ParcelFileDescriptor::new);
    let log = log.map(ParcelFileDescriptor::new);

    let vm = service.createVm(config, console.as_ref(), log.as_ref())?;

    let cid = vm.getCid()?;

    // Register callback before starting VM, in case it dies immediately.
    let state = Arc::new(Monitor::new(VmState::default()));
    let callback = BnVirtualMachineCallback::new_binder(
        VirtualMachineCallback { state: state.clone() },
        BinderFeatures::default(),
    );
    vm.registerCallback(&callback)?;
    let death_recipient = wait_for_binder_death(&mut vm.as_binder(), state.clone())?;

    vm.start()?;

    Ok(VmInstance { vm, cid, state, _death_recipient: death_recipient })
}

/// A virtual machine which has been started by the VirtualizationService.
pub struct VmInstance {
    /// The `IVirtualMachine` Binder object representing the VM.
    pub vm: Strong<dyn IVirtualMachine>,
    cid: i32,
    state: Arc<Monitor<VmState>>,
    // Ensure that the DeathRecipient isn't dropped while someone might call wait_for_completion, as
    // it is removed from the Binder when it's dropped.
    _death_recipient: DeathRecipient,
}

impl VmInstance {
    /// Returns the CID used for vsock connections to the VM.
    pub fn cid(&self) -> i32 {
        self.cid
    }

    /// Returns the current lifecycle state of the VM.
    pub fn state(&self) -> BinderResult<VirtualMachineState> {
        self.vm.getState()
    }

    /// Blocks until the VM or the VirtualizationService itself dies, and then returns the reason
    /// why it died.
    pub fn wait_for_death(&self) -> DeathReason {
        self.state.wait_while(|state| state.death_reason.is_none()).unwrap().death_reason.unwrap()
    }

    /// Waits until the VM reports that it is ready.
    ///
    /// Returns an error if the VM dies first, or the `timeout` elapses before the VM is ready.
    pub fn wait_until_ready(&self, timeout: Duration) -> Result<(), VmWaitError> {
        let (state, timeout_result) = self
            .state
            .wait_timeout_while(timeout, |state| !state.ready && state.death_reason.is_none())
            .unwrap();
        if timeout_result.timed_out() {
            Err(VmWaitError::TimedOut)
        } else if state.death_reason.is_some() {
            Err(VmWaitError::Died)
        } else {
            Ok(())
        }
    }
}

impl Debug for VmInstance {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VmInstance").field("cid", &self.cid).field("state", &self.state).finish()
    }
}

/// Notify the VmState when the given Binder object dies.
///
/// If the returned DeathRecipient is dropped then this will no longer do anything.
fn wait_for_binder_death(
    binder: &mut impl IBinder,
    state: Arc<Monitor<VmState>>,
) -> BinderResult<DeathRecipient> {
    let mut death_recipient = DeathRecipient::new(move || {
        warn!("VirtualizationService unexpectedly died");
        state.notify_death(DeathReason::VirtualizationServiceDied);
    });
    binder.link_to_death(&mut death_recipient)?;
    Ok(death_recipient)
}

#[derive(Debug, Default)]
struct VmState {
    death_reason: Option<DeathReason>,
    ready: bool,
}

impl Monitor<VmState> {
    fn notify_death(&self, reason: DeathReason) {
        let state = &mut *self.state.lock().unwrap();
        // In case this method is called more than once, ignore subsequent calls.
        if state.death_reason.is_none() {
            state.death_reason.replace(reason);
            self.cv.notify_all();
        }
    }

    fn notify_ready(&self) {
        self.state.lock().unwrap().ready = true;
        self.cv.notify_all();
    }
}

#[derive(Debug)]
struct VirtualMachineCallback {
    state: Arc<Monitor<VmState>>,
}

impl Interface for VirtualMachineCallback {}

impl IVirtualMachineCallback for VirtualMachineCallback {
    fn onPayloadStarted(
        &self,
        _cid: i32,
        _stream: Option<&ParcelFileDescriptor>,
    ) -> BinderResult<()> {
        Ok(())
    }

    fn onPayloadReady(&self, _cid: i32) -> BinderResult<()> {
        self.state.notify_ready();
        Ok(())
    }

    fn onPayloadFinished(&self, _cid: i32, _exit_code: i32) -> BinderResult<()> {
        Ok(())
    }

    fn onError(&self, _cid: i32, _error_code: i32, _message: &str) -> BinderResult<()> {
        Ok(())
    }

    fn onDied(&self, _cid: i32, reason: AidlDeathReason) -> BinderResult<()> {
        self.state.notify_death(reason.into());
        Ok(())
    }
}

/// The reason why a VM died.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeathReason {
    /// VirtualizationService died.
    VirtualizationServiceDied,
    /// There was an error waiting for the VM.
    InfrastructureError,
    /// The VM was killed.
    Killed,
    /// The VM died for an unknown reason.
    Unknown,
    /// The VM requested to shut down.
    Shutdown,
    /// crosvm had an error starting the VM.
    Error,
    /// The VM requested to reboot, possibly as the result of a kernel panic.
    Reboot,
    /// The VM or crosvm crashed.
    Crash,
    /// The pVM firmware failed to verify the VM because the public key doesn't match.
    PvmFirmwarePublicKeyMismatch,
    /// The pVM firmware failed to verify the VM because the instance image changed.
    PvmFirmwareInstanceImageChanged,
    /// The bootloader failed to verify the VM because the public key doesn't match.
    BootloaderPublicKeyMismatch,
    /// The bootloader failed to verify the VM because the instance image changed.
    BootloaderInstanceImageChanged,
    /// VirtualizationService sent a death reason which was not recognised by the client library.
    Unrecognised(AidlDeathReason),
}

impl From<AidlDeathReason> for DeathReason {
    fn from(reason: AidlDeathReason) -> Self {
        match reason {
            AidlDeathReason::INFRASTRUCTURE_ERROR => Self::InfrastructureError,
            AidlDeathReason::KILLED => Self::Killed,
            AidlDeathReason::UNKNOWN => Self::Unknown,
            AidlDeathReason::SHUTDOWN => Self::Shutdown,
            AidlDeathReason::ERROR => Self::Error,
            AidlDeathReason::REBOOT => Self::Reboot,
            AidlDeathReason::CRASH => Self::Crash,
            AidlDeathReason::PVM_FIRMWARE_PUBLIC_KEY_MISMATCH => Self::PvmFirmwarePublicKeyMismatch,
            AidlDeathReason::PVM_FIRMWARE_INSTANCE_IMAGE_CHANGED => {
                Self::PvmFirmwareInstanceImageChanged
            }
            AidlDeathReason::BOOTLOADER_PUBLIC_KEY_MISMATCH => Self::BootloaderPublicKeyMismatch,
            AidlDeathReason::BOOTLOADER_INSTANCE_IMAGE_CHANGED => {
                Self::BootloaderInstanceImageChanged
            }
            _ => Self::Unrecognised(reason),
        }
    }
}

impl Display for DeathReason {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let s = match self {
            Self::VirtualizationServiceDied => "VirtualizationService died.",
            Self::InfrastructureError => "Error waiting for VM to finish.",
            Self::Killed => "VM was killed.",
            Self::Unknown => "VM died for an unknown reason.",
            Self::Shutdown => "VM shutdown cleanly.",
            Self::Error => "Error starting VM.",
            Self::Reboot => "VM tried to reboot, possibly due to a kernel panic.",
            Self::Crash => "VM crashed.",
            Self::PvmFirmwarePublicKeyMismatch => {
                "pVM firmware failed to verify the VM because the public key doesn't match."
            }
            Self::PvmFirmwareInstanceImageChanged => {
                "pVM firmware failed to verify the VM because the instance image changed."
            }
            Self::BootloaderPublicKeyMismatch => {
                "Bootloader failed to verify the VM because the public key doesn't match."
            }
            Self::BootloaderInstanceImageChanged => {
                "Bootloader failed to verify the VM because the instance image changed."
            }
            Self::Unrecognised(reason) => {
                return write!(f, "Unrecognised death reason {:?}.", reason);
            }
        };
        f.write_str(s)
    }
}
