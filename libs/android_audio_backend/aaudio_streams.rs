/*
 * Copyright 2024 The Android Open Source Project
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

use std::os::raw::{c_int, c_uint};
use std::ptr::addr_of_mut;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;
use audio_streams::AsyncBufferCommit;
use audio_streams::AsyncPlaybackBuffer;
use audio_streams::AsyncPlaybackBufferStream;
use audio_streams::AudioStreamsExecutor;
use audio_streams::BoxError;
use audio_streams::BufferCommit;
use audio_streams::PlaybackBuffer;
use audio_streams::PlaybackBufferStream;
use audio_streams::SampleFormat;
use audio_streams::StreamControl;
use audio_streams::StreamSource;
use audio_streams::StreamSourceGenerator;

enum AAudioStream {}
enum AAudioStreamBuilder {}

extern "C" {
    fn AAudio_createStreamBuilder(builder: *mut *mut AAudioStreamBuilder) -> c_int;
    fn AAudioStreamBuilder_setFormat(builder: *mut AAudioStreamBuilder, format: c_int) -> c_int;
    fn AAudioStreamBuilder_setSampleRate(
        builder: *mut AAudioStreamBuilder,
        sampleRate: c_uint,
    ) -> c_int;
    fn AAudioStreamBuilder_setChannelCount(
        builder: *mut AAudioStreamBuilder,
        channelCount: c_int,
    ) -> c_int;
    fn AAudioStreamBuilder_openStream(
        builder: *mut AAudioStreamBuilder,
        stream: *mut *mut AAudioStream,
    ) -> c_int;
    fn AAudioStream_requestStart(stream: *mut AAudioStream) -> c_int;
    fn AAudioStream_write(
        stream: *mut AAudioStream,
        buffer: *const u8,
        numFrames: c_int,
        timeoutNanos: c_int,
    ) -> c_int;
    fn AAudioStream_release(stream: *mut AAudioStream) -> c_int;
}

struct AaudioStream {
    buffer: Vec<u8>,
    frame_size: usize,
    interval: Duration,
    next_frame: Duration,
    start_time: Option<Instant>,
    // According to https://developer.android.com/ndk/guides/audio/aaudio/aaudio#thread-safety,
    // the AAudioStream is not thread-safe. A mutex is needed it is used with async functions.
    stream: Mutex<*mut AAudioStream>,
}

// SAFETY:
// Mutex<*mut AAudioStream> is thread-safe
unsafe impl Send for AaudioStream {}
// SAFETY:
// Mutex<*mut AAudioStream> is thread-safe
unsafe impl Sync for AaudioStream {}

impl BufferCommit for AaudioStream {
    fn commit(&mut self, _nwritten: usize) {
        unimplemented!();
    }
}

#[async_trait(?Send)]
impl AsyncBufferCommit for AaudioStream {
    async fn commit(&mut self, nwritten: usize) {
        // SAFETY:
        // The AAudioStream_write reads buffer for nwritten * frame_size bytes
        // It is safe since nwritten < buffer_size and the buffer.len() == buffer_size * frame_size
        unsafe {
            AAudioStream_write(
                *self.stream.lock().unwrap(),
                self.buffer.as_ptr(),
                nwritten as c_int,
                0,
            );
        }
    }
}

impl AaudioStream {
    pub fn new(
        num_channels: usize,
        format: SampleFormat,
        frame_rate: u32,
        buffer_size: usize,
    ) -> Self {
        let frame_size = format.sample_bytes() * num_channels;
        let interval = Duration::from_millis(buffer_size as u64 * 1000 / frame_rate as u64);

        let mut _stream: *mut AAudioStream = std::ptr::null_mut();
        let mut builder: *mut AAudioStreamBuilder = std::ptr::null_mut();

        // SAFETY:
        // Interfacing with the AAudio C API. Assumes correct linking
        // and `builder` and `stream` pointers are valid and properly initialized.
        unsafe {
            AAudio_createStreamBuilder(&mut builder);
            AAudioStreamBuilder_setFormat(builder, format as c_int);
            AAudioStreamBuilder_setSampleRate(builder, frame_rate as c_uint);
            AAudioStreamBuilder_setChannelCount(builder, num_channels as c_int);

            AAudioStreamBuilder_openStream(builder, addr_of_mut!(_stream));
            AAudioStream_requestStart(_stream);
        }
        AaudioStream {
            buffer: vec![0; buffer_size * frame_size],
            frame_size,
            interval,
            next_frame: interval,
            start_time: None,
            stream: Mutex::new(_stream),
        }
    }
}

impl PlaybackBufferStream for AaudioStream {
    fn next_playback_buffer<'b, 's: 'b>(&'s mut self) -> Result<PlaybackBuffer<'b>, BoxError> {
        unimplemented!();
    }
}

#[async_trait(?Send)]
impl AsyncPlaybackBufferStream for AaudioStream {
    async fn next_playback_buffer<'a>(
        &'a mut self,
        ex: &dyn AudioStreamsExecutor,
    ) -> Result<AsyncPlaybackBuffer<'a>, BoxError> {
        if let Some(start_time) = self.start_time {
            let elapsed = start_time.elapsed();
            if elapsed < self.next_frame {
                ex.delay(self.next_frame - elapsed).await?;
            }
            self.next_frame += self.interval;
        } else {
            self.start_time = Some(Instant::now());
            self.next_frame = self.interval;
        }
        // SAFETY:
        // self.buffer.as_mut_ptr() is a valid pointer
        let slice =
            unsafe { std::slice::from_raw_parts_mut(self.buffer.as_mut_ptr(), self.buffer.len()) };
        Ok(AsyncPlaybackBuffer::new(self.frame_size, slice, self)?)
    }
}

impl Drop for AaudioStream {
    fn drop(&mut self) {
        // SAFETY:
        // Interfacing with the AAudio C API. Assumes correct linking
        // and `stream` are valid and properly initialized.
        unsafe {
            AAudioStream_release(*self.stream.lock().unwrap());
        }
    }
}

#[derive(Default)]
struct AaudioStreamControl;

impl AaudioStreamControl {
    pub fn new() -> Self {
        AaudioStreamControl {}
    }
}

impl StreamControl for AaudioStreamControl {}

#[derive(Default)]
struct AaudioStreamSource;

impl StreamSource for AaudioStreamSource {
    #[allow(clippy::type_complexity)]
    fn new_playback_stream(
        &mut self,
        _num_channels: usize,
        _format: SampleFormat,
        _frame_rate: u32,
        _buffer_size: usize,
    ) -> Result<(Box<dyn StreamControl>, Box<dyn PlaybackBufferStream>), BoxError> {
        unimplemented!();
    }

    #[allow(clippy::type_complexity)]
    fn new_async_playback_stream(
        &mut self,
        num_channels: usize,
        format: SampleFormat,
        frame_rate: u32,
        buffer_size: usize,
        _ex: &dyn AudioStreamsExecutor,
    ) -> Result<(Box<dyn StreamControl>, Box<dyn AsyncPlaybackBufferStream>), BoxError> {
        Ok((
            Box::new(AaudioStreamControl::new()),
            Box::new(AaudioStream::new(num_channels, format, frame_rate, buffer_size)),
        ))
    }
}

pub struct AaudioStreamSourceGenerator;

impl AaudioStreamSourceGenerator {
    pub fn new() -> Self {
        AaudioStreamSourceGenerator {}
    }
}

/// `AaudioStreamSourceGenerator` is a struct that implements [`StreamSourceGenerator`]
/// for `AaudioStreamSource`.
impl StreamSourceGenerator for AaudioStreamSourceGenerator {
    fn generate(&self) -> Result<Box<dyn StreamSource>, BoxError> {
        Ok(Box::new(AaudioStreamSource))
    }
}

impl Default for AaudioStreamSourceGenerator {
    fn default() -> Self {
        Self::new()
    }
}
