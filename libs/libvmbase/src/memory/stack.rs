// Copyright 2024, The Android Open Source Project
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

//! Low-level stack support.

/// Configures the maximum size of the stack.
#[macro_export]
macro_rules! limit_stack_size {
    ($len:expr) => {
        #[export_name = "vmbase_max_stack_size"]
        fn __vmbase_max_stack_size() -> Option<usize> {
            Some($len)
        }
    };
}

// TODO(ptosi): Find a way to remove the need for this macro when no limit is necessary.
/// Let vmbase make the stack region as large as possible.
#[macro_export]
macro_rules! unlimited_stack_size {
    () => {
        #[export_name = "vmbase_max_stack_size"]
        fn __vmbase_max_stack_size() -> Option<usize> {
            None
        }
    };
}

extern "Rust" {
    fn vmbase_max_stack_size() -> Option<usize>;
}

pub(crate) fn max_stack_size() -> Option<usize> {
    // SAFETY: Function is either provided by client or defaults to our weak implementation.
    unsafe { vmbase_max_stack_size() }
}
