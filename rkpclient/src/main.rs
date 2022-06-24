/*
 * Copyright (C) 2022 The Android Open Source Project
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

//! Mock up of an client to the RKP VM for VMs.

use android_security_dice::aidl::android::security::dice::IDiceNode::IDiceNode;
use android_system_virtualizationcommon::aidl::android::system::virtualizationcommon::ErrorCode::ErrorCode;
use android_system_virtualmachineservice::{
    aidl::android::system::virtualmachineservice::IVirtualMachineService::{
        IVirtualMachineService, VM_BINDER_SERVICE_PORT,
    },
    binder::Strong,
};
use anyhow::{anyhow, Context, Result};
use binder::wait_for_interface;
use coset::iana::Algorithm;
use coset::{CborSerializable, CoseSign1Builder, HeaderBuilder};
use diced_open_dice_cbor::{ContextImpl, OpenDiceCborContext};
use log::{debug, error};
use openssl::pkey::{Id, PKey, Private};
use openssl::sign::Signer;
use rpcbinder::get_vsock_rpc_interface;
use std::os::raw::c_int;
use std::panic;

/// The CID representing the host VM
const VMADDR_CID_HOST: u32 = 2;

/// Entry point called from microdroid_launcher.
#[no_mangle]
pub extern "C" fn android_native_main(_argc: c_int, _argv: *const *const char) -> c_int {
    if let Err(e) = try_main() {
        error!("failed with {:?}", e);
        let vm_service = get_vm_service().unwrap();
        vm_service.notifyError(ErrorCode::UNKNOWN, &e.to_string()).unwrap();
        loop {
            std::thread::sleep(std::time::Duration::new(5, 0));
        }
    }
    0
}

fn get_key() -> Result<(Vec<u8>, PKey<Private>)> {
    let diced = wait_for_interface::<dyn IDiceNode>("android.security.dice.IDiceNode")
        .context("IDiceNode service not found")?;
    let bcc_handover = diced.derive(&[]).context("Failed to get BccHandover")?;
    let mut dice = OpenDiceCborContext::new();
    let seed = dice.derive_cdi_private_key_seed(&bcc_handover.cdiAttest)?;
    let attestation_key = PKey::private_key_from_raw_bytes(&seed, Id::ED25519)?;
    Ok((bcc_handover.bcc.data, attestation_key))
}

fn try_main() -> Result<()> {
    android_logger::init_once(
        android_logger::Config::default().with_tag("rkpclient").with_min_level(log::Level::Debug),
    );
    // Redirect panic messages to logcat.
    panic::set_hook(Box::new(|panic_info| {
        error!("{}", panic_info);
    }));

    let vm_service = get_vm_service()?;

    debug!("rkpclient is starting as a rpc service.");
    if let Err(e) = vm_service.notifyPayloadReady() {
        error!("Unable to notify ready: {}", e);
    }

    debug!("rkpclient is going to proxy.");
    let (bcc, attestation_key) = get_key()?;

    let key = PKey::generate_ed25519()?;
    let sign1 = CoseSign1Builder::new()
        .protected(HeaderBuilder::new().algorithm(Algorithm::EdDSA).build())
        .payload(key.public_key_to_der()?)
        .try_create_signature(&[], |m| {
            Signer::new_without_digest(&attestation_key)?.sign_oneshot_to_vec(m)
        })
        .context("Creating COSE_Sign1")?
        .build()
        .to_vec()
        .map_err(|ce| anyhow!("Creating COSE_Sign1: {:?}", ce))?;

    let challenge = b"This is from the server";
    let _certificates = vm_service
        .getRemotelyAttestedCertificate(&bcc, &sign1, challenge)
        .context("Unable to proxy to rkp")?;
    debug!("rkpclient is done trying to proxy.");

    Ok(())
}

fn get_vm_service() -> Result<Strong<dyn IVirtualMachineService>> {
    get_vsock_rpc_interface(VMADDR_CID_HOST, VM_BINDER_SERVICE_PORT as u32)
        .context("Failed to connect to IVirtualMachineService")
}
