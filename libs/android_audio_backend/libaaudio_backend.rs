#![allow(missing_docs)]

#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("cxx_aaudio_backend.hpp");
        unsafe fn aaudio_init(num_channels: usize, framte_rate: u32);
        unsafe fn aaudio_playback(buffer: &mut [u8], num_frame: usize);
        unsafe fn aaudio_release();
    }
}

pub fn init(num_channels: usize, frame_rate: u32) {
    // SAFETY:
    // These two arguments is read-only in this extern cpp call.
    unsafe {
        ffi::aaudio_init(num_channels, frame_rate);
    }
}

/// # Safety
/// This function should not be called before the horsemen are ready.
pub fn playback(buffer: &mut [u8], num_frame: usize) {
    // SAFETY:
    // This only calls AAudioStream_write.
    unsafe { ffi::aaudio_playback(buffer, num_frame) };
}

pub fn release() {
    // SAFETY:
    // This only calls AAudioStream_release.
    unsafe { ffi::aaudio_release() };
}
