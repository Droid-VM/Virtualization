#pragma once

#include "rust/cxx.h"
#include "libaaudio_backend.rs.h"
#include <aaudio/AAudio.h>

void aaudio_init(size_t num_channel, uint32_t frame_rate);
void aaudio_playback(rust::Slice<uint8_t> buffer, size_t numFrame);
void aaudio_release();
