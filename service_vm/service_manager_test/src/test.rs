/*
 * Copyright (C) 2024 The Android Open Source Project
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

//! Test service manager for VM AIDL services change.
use com_android_virt_vm_attestation_testservice::aidl::com::android::virt::vm_attestation::testservice::IAttestationService::IAttestationService;
use rdroidtest::rdroidtest;

const FOO_SERVICE: &str = "com.android.virt.vm_attestation.testservice.IAttestationService/default";

fn foo_service() -> binder::Strong<dyn IAttestationService> {
    binder::wait_for_interface(FOO_SERVICE).unwrap()
}

#[rdroidtest]
fn getting_foo_service_succeeds() {
    let service = foo_service();
    // This call should panic in virtualizationservice.
    service.requestAttestationForTesting().unwrap();
}

rdroidtest::test_main!();
