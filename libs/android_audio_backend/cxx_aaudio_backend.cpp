#include "cxx_aaudio_backend.hpp"

#include "libaaudio_backend.rs.h"

AAudioStream *stream = nullptr;

void aaudio_init(size_t num_channel, uint32_t frame_rate) {
    AAudioStreamBuilder *builder;
    aaudio_result_t result;
    result = AAudio_createStreamBuilder(&builder);
    AAudioStreamBuilder_setFormat(builder, AAUDIO_FORMAT_PCM_I16);
    AAudioStreamBuilder_setSampleRate(builder, frame_rate);
    AAudioStreamBuilder_setChannelCount(builder, num_channel);
    result = AAudioStreamBuilder_openStream(builder, &stream);
    result = AAudioStream_requestStart(stream);
}

void aaudio_playback(rust::Slice<uint8_t> buffer, size_t num_frame) {
    AAudioStream_write(stream, buffer.data(), num_frame, 0);
}

void aaudio_release() {
    AAudioStream_release(stream);
}
