// Copyright 2023, The Android Open Source Project
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

//! Helper wrapper around RKPD interface

use crate::REMOTELY_PROVISIONED_COMPONENT_SERVICE_NAME;
use android_security_rkp_aidl::aidl::android::security::rkp::{
    IGetKeyCallback::BnGetKeyCallback, IGetKeyCallback::ErrorCode::ErrorCode as GetKeyErrorCode,
    IGetKeyCallback::IGetKeyCallback, IGetRegistrationCallback::BnGetRegistrationCallback,
    IGetRegistrationCallback::IGetRegistrationCallback, IRegistration::IRegistration,
    IRemoteProvisioning::IRemoteProvisioning, RemotelyProvisionedKey::RemotelyProvisionedKey,
};
use anyhow::{anyhow, Context, Result};
use binder::{wait_for_interface, BinderFeatures, Interface, Strong};
use log::{error, warn};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::time::Duration;

const REMOTE_PROVISIONING_SERVICE_NAME: &str = "remote_provisioning";
const RKPD_TIMEOUT: Duration = Duration::from_secs(10);

/// Thread-safe channel for sending values.
struct SafeSender<T> {
    inner: Mutex<Sender<T>>,
}

/// Thread-safe channel for sending value.
impl<T> SafeSender<T> {
    fn new(sender: Sender<T>) -> Self {
        Self { inner: Mutex::new(sender) }
    }

    fn send(&self, value: T) {
        if let Err(e) = self.inner.lock().unwrap().send(value) {
            error!("Failed to send value: {e:?}");
        }
    }
}

pub(crate) fn get_attestation_key() -> Result<RemotelyProvisionedKey> {
    let registration = get_rkpd_registration(REMOTELY_PROVISIONED_COMPONENT_SERVICE_NAME)?;
    let (tx, rx) = channel();
    let get_key_callback = GetKeyCallback::new_binder(tx);

    // TODO(b/241428146): Use the correct key ID.
    let key_id = 0;
    registration.getKey(key_id, &get_key_callback).context("Failed to get key.")?;
    rx.recv_timeout(RKPD_TIMEOUT).context("Timeout waiting for the key")?
}

fn get_rkpd_registration(registration_name: &str) -> Result<Strong<dyn IRegistration>> {
    let remote_provisioning_service = get_remote_provisioning_service()?;
    let (tx, rx) = channel();
    let get_registration_callback = GetRegistrationCallback::new_binder(tx);

    remote_provisioning_service
        .getRegistration(registration_name, &get_registration_callback)
        .context("Failed to get registration")?;
    rx.recv_timeout(RKPD_TIMEOUT).context("Timeout waiting for the registration")?
}

fn get_remote_provisioning_service() -> Result<Strong<dyn IRemoteProvisioning>> {
    wait_for_interface(REMOTE_PROVISIONING_SERVICE_NAME)
        .context(format!("Failed to connect to service: {}", REMOTE_PROVISIONING_SERVICE_NAME))
}

struct GetRegistrationCallback {
    registration_tx: SafeSender<Result<Strong<dyn IRegistration>>>,
}

impl GetRegistrationCallback {
    pub fn new_binder(
        registration_tx: Sender<Result<Strong<dyn IRegistration>>>,
    ) -> Strong<dyn IGetRegistrationCallback> {
        let result = GetRegistrationCallback { registration_tx: SafeSender::new(registration_tx) };
        BnGetRegistrationCallback::new_binder(result, BinderFeatures::default())
    }
}

impl Interface for GetRegistrationCallback {}

impl IGetRegistrationCallback for GetRegistrationCallback {
    fn onSuccess(&self, registration: &Strong<dyn IRegistration>) -> binder::Result<()> {
        self.registration_tx.send(Ok(registration.clone()));
        Ok(())
    }
    fn onCancel(&self) -> binder::Result<()> {
        warn!("IGetRegistrationCallback cancelled");
        self.registration_tx.send(Err(anyhow!("GetRegistrationCallback cancelled.")));
        Ok(())
    }
    fn onError(&self, description: &str) -> binder::Result<()> {
        error!("IGetRegistrationCallback failed: '{description}'");
        self.registration_tx
            .send(Err(anyhow!("GetRegistrationCallback failed: {:?}", description)));
        Ok(())
    }
}

struct GetKeyCallback {
    key_tx: SafeSender<Result<RemotelyProvisionedKey>>,
}

impl GetKeyCallback {
    pub fn new_binder(
        key_tx: Sender<Result<RemotelyProvisionedKey>>,
    ) -> Strong<dyn IGetKeyCallback> {
        let result = GetKeyCallback { key_tx: SafeSender::new(key_tx) };
        BnGetKeyCallback::new_binder(result, BinderFeatures::default())
    }
}

impl Interface for GetKeyCallback {}

impl IGetKeyCallback for GetKeyCallback {
    fn onSuccess(&self, key: &RemotelyProvisionedKey) -> binder::Result<()> {
        self.key_tx.send(Ok(RemotelyProvisionedKey {
            keyBlob: key.keyBlob.clone(),
            encodedCertChain: key.encodedCertChain.clone(),
        }));
        Ok(())
    }
    fn onCancel(&self) -> binder::Result<()> {
        warn!("IGetKeyCallback cancelled");
        self.key_tx.send(Err(anyhow!("GetKeyCallback cancelled.")));
        Ok(())
    }
    fn onError(&self, error: GetKeyErrorCode, description: &str) -> binder::Result<()> {
        error!("IGetKeyCallback failed: {:?} {:?}", error, description);
        self.key_tx.send(Err(anyhow!("GetKeyCallback failed: {:?} {:?}", error, description)));
        Ok(())
    }
}
