use std::ptr;

use super::Delay;
use ffi::*;
use libc::c_int;
use std::ffi::c_void;
use util::format;
use Dictionary;
use {frame, ChannelLayout, Error};

#[derive(Eq, PartialEq, Copy, Clone)]
pub struct Definition {
    pub format: format::Sample,
    pub channel_layout: ChannelLayout,
    pub rate: u32,
}

pub struct Context {
    ptr: *mut SwrContext,

    input: Definition,
    output: Definition,
}

unsafe impl Send for Context {}

impl Context {
    #[doc(hidden)]
    pub unsafe fn as_ptr(&self) -> *const SwrContext {
        self.ptr as *const _
    }

    #[doc(hidden)]
    pub unsafe fn as_mut_ptr(&mut self) -> *mut SwrContext {
        self.ptr
    }
}

impl Context {
    /// Create a resampler with the given definitions.
    pub fn get(
        src_format: format::Sample,
        src_channel_layout: ChannelLayout,
        src_rate: u32,
        dst_format: format::Sample,
        dst_channel_layout: ChannelLayout,
        dst_rate: u32,
    ) -> Result<Self, Error> {
        Self::get_with(
            src_format,
            src_channel_layout,
            src_rate,
            dst_format,
            dst_channel_layout,
            dst_rate,
            Dictionary::new(),
        )
    }

    /// Create a resampler with the given definitions and custom options dictionary.
    pub fn get_with(
        src_format: format::Sample,
        src_channel_layout: ChannelLayout,
        src_rate: u32,
        dst_format: format::Sample,
        dst_channel_layout: ChannelLayout,
        dst_rate: u32,
        options: Dictionary,
    ) -> Result<Self, Error> {
        unsafe {
            #[allow(unused_assignments)]
            let mut ptr = std::ptr::null_mut();

            #[cfg(not(feature = "ffmpeg_7_0"))]
            {
                ptr = swr_alloc_set_opts(
                    ptr::null_mut(),
                    dst_channel_layout.bits() as i64,
                    dst_format.into(),
                    dst_rate as c_int,
                    src_channel_layout.bits() as i64,
                    src_format.into(),
                    src_rate as c_int,
                    0,
                    ptr::null_mut(),
                );
            }
            #[cfg(feature = "ffmpeg_7_0")]
            {
                let e = swr_alloc_set_opts2(
                    &mut ptr,
                    &dst_channel_layout.into(),
                    dst_format.into(),
                    dst_rate as c_int,
                    &src_channel_layout.into(),
                    src_format.into(),
                    src_rate as c_int,
                    0,
                    ptr::null_mut(),
                );
                if e != 0 {
                    return Err(Error::from(e));
                }
            }

            let mut opts = options.disown();
            let res = av_opt_set_dict(ptr as *mut c_void, &mut opts);
            Dictionary::own(opts);

            if res != 0 {
                return Err(Error::from(res));
            }

            if !ptr.is_null() {
                match swr_init(ptr) {
                    e if e < 0 => Err(Error::from(e)),

                    _ => Ok(Context {
                        ptr,

                        input: Definition {
                            format: src_format,
                            channel_layout: src_channel_layout,
                            rate: src_rate,
                        },

                        output: Definition {
                            format: dst_format,
                            channel_layout: dst_channel_layout,
                            rate: dst_rate,
                        },
                    }),
                }
            } else {
                Err(Error::InvalidData)
            }
        }
    }

    /// Get the input definition.
    pub fn input(&self) -> &Definition {
        &self.input
    }

    /// Get the output definition.
    pub fn output(&self) -> &Definition {
        &self.output
    }

    /// Get the remaining delay.
    pub fn delay(&self) -> Option<Delay> {
        unsafe {
            match swr_get_delay(self.as_ptr() as *mut _, 1) {
                0 => None,
                _ => Some(Delay::from(self)),
            }
        }
    }

    /// Run the resampler from the given input to the given output.
    ///
    /// When there are internal frames to process it will return `Ok(Some(Delay { .. }))`.
    pub fn run(
        &mut self,
        input: &frame::Audio,
        output: &mut frame::Audio,
    ) -> Result<Option<Delay>, Error> {
        unsafe {
            (*output.as_mut_ptr()).sample_rate = self.output.rate as i32;
        }

        unsafe {
            if output.is_empty() {
                output.alloc(
                    self.output.format,
                    input.samples(),
                    self.output.channel_layout,
                );
            }

            match swr_convert_frame(self.as_mut_ptr(), output.as_mut_ptr(), input.as_ptr()) {
                0 => Ok(self.delay()),

                e => Err(Error::from(e)),
            }
        }
    }

    /// Lower-level conversion using `swr_convert` instead of `swr_convert_frame`.
    /// `swr_convert_frame` checks that the frame's metadata fields (`ch_layout`,
    /// `sample_rate`, `format`) match what the context was initialized with, and on
    /// Windows static FFmpeg 7.x builds those fields are often left zeroed by the AAC
    /// decoder even though the PCM data itself is valid. `swr_convert` skips that check.
    ///
    /// `in_planes` is one `*const u8` per input channel (planar format); pass an empty
    /// slice with `in_samples = 0` to flush the internal SWR delay buffer.
    /// The output is always written as two-channel planar f32 into `out_left`/`out_right`.
    /// Returns the number of samples per channel written.
    pub fn convert_planes(
        &mut self,
        in_planes: &[*const u8],
        in_samples: i32,
        out_left: &mut [f32],
        out_right: &mut [f32],
    ) -> Result<usize, Error> {
        debug_assert_eq!(out_left.len(), out_right.len());
        unsafe {
            let mut out_ptrs = [
                out_left.as_mut_ptr() as *mut u8,
                out_right.as_mut_ptr() as *mut u8,
            ];
            let in_ptr = if in_samples > 0 { in_planes.as_ptr() } else { ptr::null() };
            let n = swr_convert(
                self.ptr,
                out_ptrs.as_mut_ptr(),
                out_left.len() as c_int,
                in_ptr,
                in_samples,
            );
            if n < 0 {
                Err(Error::from(n))
            } else {
                Ok(n as usize)
            }
        }
    }

    /// Estimated output sample count for the given input count.
    pub fn out_sample_count(&self, in_samples: i32) -> i32 {
        unsafe { swr_get_out_samples(self.ptr as *mut _, in_samples).max(0) }
    }

    /// Convert one of the remaining internal frames.
    ///
    /// When there are no more internal frames `Ok(None)` will be returned.
    pub fn flush(&mut self, output: &mut frame::Audio) -> Result<Option<Delay>, Error> {
        unsafe {
            (*output.as_mut_ptr()).sample_rate = self.output.rate as i32;
        }

        unsafe {
            match swr_convert_frame(self.as_mut_ptr(), output.as_mut_ptr(), ptr::null()) {
                0 => Ok(self.delay()),

                e => Err(Error::from(e)),
            }
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            swr_free(&mut self.as_mut_ptr());
        }
    }
}
