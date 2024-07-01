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

//! Manages running instances of the Microfuchsia VM.
//! At most one instance should be running at a time.

use crate::instance_starter::{InstanceStarter, MicrofuchsiaInstance};
use android_system_virtualizationservice::aidl::android::system::virtualizationservice;
use anyhow::{bail, Result};
use binder::Strong;
use std::sync::{Arc, Mutex, Weak};
use virtualizationservice::IVirtualizationService::IVirtualizationService;

pub struct InstanceManager {
    service: Strong<dyn IVirtualizationService>,
    state: Mutex<State>,
}

impl InstanceManager {
    pub fn new(service: Strong<dyn IVirtualizationService>) -> Self {
        Self { service, state: Default::default() }
    }

    pub fn start_instance(&self) -> Result<MicrofuchsiaInstance> {
        let mut state = self.state.lock().unwrap();
        state.mark_starting()?;
        // Don't hold the lock while we start the instance to avoid blocking other callers.
        drop(state);

        let instance_starter = InstanceStarter::new("Microfuchsia", 0);
        let instance = instance_starter.start_new_instance(&*self.service);

        let mut state = self.state.lock().unwrap();
        if let Ok(ref instance) = instance {
            state.mark_started(instance.get_instance_tracker())?;
        } else {
            state.mark_stopped();
        }
        instance
    }
}

// Ensures we only run one instance at a time.
// Valid states:
// Starting: is_starting is true, instance_tracker is None.
// Started: is_starting is false, instance_tracker is Some(x) and there is a strong ref to x.
// Stopped: is_starting is false and instance_tracker is None or a weak ref to a dropped instance.
// The panic calls here should never happen, unless the code above in InstanceManager is buggy.
// In particular nothing the client does should be able to trigger them.
#[derive(Default)]
struct State {
    instance_tracker: Option<Weak<()>>,
    is_starting: bool,
}

impl State {
    // Move to Starting iff we are Stopped.
    fn mark_starting(&mut self) -> Result<()> {
        if self.is_starting {
            bail!("An instance is already starting");
        }
        if let Some(weak) = &self.instance_tracker {
            if weak.strong_count() != 0 {
                bail!("An instance is already running");
            }
        }
        self.instance_tracker = None;
        self.is_starting = true;
        Ok(())
    }

    // Move from Starting to Stopped.
    fn mark_stopped(&mut self) {
        if !self.is_starting || self.instance_tracker.is_some() {
            panic!("Tried to mark stopped when not starting");
        }
        self.is_starting = false;
    }

    // Move from Starting to Started.
    fn mark_started(&mut self, instance_tracker: &Arc<()>) -> Result<()> {
        if !self.is_starting {
            panic!("Tried to mark started when not starting")
        }
        if self.instance_tracker.is_some() {
            panic!("Attempted to mark started when already started");
        }
        self.is_starting = false;
        self.instance_tracker = Some(Arc::downgrade(instance_tracker));
        Ok(())
    }
}
