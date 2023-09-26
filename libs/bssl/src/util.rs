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

//! Utility functions.

use bssl_avf_error::{ApiName, Error, Result};

pub(crate) fn check_int_result(ret: i32, api_name: ApiName) -> Result<()> {
    if ret == 1 {
        Ok(())
    } else {
        assert_eq!(ret, 0, "Unexpected return value {ret} for {api_name:?}");
        Err(Error::CallFailed(api_name))
    }
}
