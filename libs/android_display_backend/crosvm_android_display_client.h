// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

extern "C" {

typedef void (*android_display_log_callback_type)(const char* message);

static void android_display_log_callback_stub(const char* message) {
    (void)message;
}

struct android_display_context {
    uint32_t test;
};

__attribute__((visibility("default"))) struct android_display_context*
create_android_display_context(const char* name, size_t name_len,
                               android_display_log_callback_type error_callback);

__attribute__((visibility("default"))) void destroy_android_display_context(
        android_display_log_callback_type error_callback, struct android_display_context* ctx);

} // extern C
