//! Decodes a video file's own audio track (if any) fully upfront and plays it back through a
//! `cpal` output stream, driven by the interactive app's own transport position — video is
//! always the master clock (same principle as the existing MIDI sync design: `midi_time =
//! position - sync_offset`), audio is never allowed to run its own independent clock, so it
//! can't drift out of sync with the displayed frame over a long playback. For a several-minute
//! piano performance video, decoding the whole track upfront into memory is a bounded,
//! already-accepted cost — `crates/export`'s own audio mux does exactly this for the same
//! reason.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::sample::{Sample, Type};
use ffmpeg_next::software::resampling::context::Context as ResamplingContext;
use ffmpeg_next::util::frame::audio::Audio as AudioFrame;

const RESYNC_THRESHOLD_SECONDS: f64 = 0.050;

/// Bucket width for the downsampled waveform summary handed to the UI's timeline — fine enough
/// that scrubbing/zooming still shows real detail, coarse enough that a several-minute track
/// produces a peaks array in the tens of thousands of entries rather than millions.
const WAVEFORM_BUCKET_SECONDS: f64 = 0.01;

pub struct AudioPlayback {
    stream: Option<cpal::Stream>,
    /// Transport position in seconds (as `f64::to_bits`), updated once per redraw by
    /// `set_position_seconds`. The output callback uses it as a resync anchor while advancing
    /// its own sample cursor between callbacks; app redraws run at video cadence, which is too
    /// sparse to be used as a per-buffer audio seek target.
    position_bits: Arc<AtomicU64>,
    /// Incremented for explicit seeks/scrubs so even a tiny user-driven jump resyncs the audio
    /// cursor instead of being treated like harmless clock drift.
    resync_generation: Arc<AtomicU64>,
    /// Peak amplitude (0.0-1.0) per `WAVEFORM_BUCKET_SECONDS`-wide bucket across the whole
    /// track, computed once at load time — the UI's timeline draws this directly rather than
    /// re-scanning raw samples every redraw.
    waveform_peaks: Vec<f32>,
}

impl Default for AudioPlayback {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPlayback {
    pub fn new() -> Self {
        Self {
            stream: None,
            position_bits: Arc::new(AtomicU64::new(0)),
            resync_generation: Arc::new(AtomicU64::new(0)),
            waveform_peaks: Vec::new(),
        }
    }

    /// Peak amplitude (0.0-1.0) per `waveform_bucket_seconds()`-wide bucket across the whole
    /// track, empty if nothing with an audio stream has been loaded.
    pub fn waveform_peaks(&self) -> &[f32] {
        &self.waveform_peaks
    }

    /// Width in seconds of each `waveform_peaks()` entry.
    pub fn waveform_bucket_seconds(&self) -> f64 {
        WAVEFORM_BUCKET_SECONDS
    }

    /// True once a track with a real audio stream has been successfully `load`ed.
    pub fn is_active(&self) -> bool {
        self.stream.is_some()
    }

    /// Cheap probe so callers can decide whether it's worth loading at all. Duplicated from
    /// (rather than shared with) `crates/export/src/audio.rs`'s own `has_audio_stream` — that
    /// crate's `audio` module is private, and this crate has no other reason to depend on
    /// `export`.
    pub fn has_audio_stream(path: &Path) -> bool {
        ffmpeg::format::input(path)
            .ok()
            .and_then(|input| input.streams().best(ffmpeg::media::Type::Audio).map(|_| ()))
            .is_some()
    }

    /// Decodes `path`'s audio track (if any) and starts a new output stream for it, replacing
    /// any previous one. A video with no audio stream leaves playback inactive
    /// (`is_active() == false`) rather than erroring — it just stays silent, no different from
    /// a video that happens to have one.
    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        // Drop the old stream (if any) before decoding the new one, rather than after building
        // the replacement — two streams briefly fighting the same output device is pointless
        // even if harmless.
        self.stream = None;
        self.position_bits
            .store(0.0_f64.to_bits(), Ordering::Relaxed);
        self.resync_generation.store(0, Ordering::Relaxed);
        self.waveform_peaks = Vec::new();

        if !Self::has_audio_stream(path) {
            return Ok(());
        }

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output device".to_string())?;
        let config = device
            .default_output_config()
            .map_err(|err| format!("failed to get default audio output config: {err}"))?;

        let decoded = decode_all(path, config.sample_rate() as i32)?;
        self.waveform_peaks =
            compute_waveform_peaks(&decoded.left, &decoded.right, decoded.sample_rate);
        let channels = config.channels() as usize;
        let stream_config: cpal::StreamConfig = config.into();
        let position_bits = self.position_bits.clone();
        let resync_generation = self.resync_generation.clone();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build_typed_stream::<f32>(
                &device,
                stream_config,
                channels,
                position_bits,
                resync_generation,
                decoded,
            ),
            cpal::SampleFormat::I16 => build_typed_stream::<i16>(
                &device,
                stream_config,
                channels,
                position_bits,
                resync_generation,
                decoded,
            ),
            cpal::SampleFormat::U16 => build_typed_stream::<u16>(
                &device,
                stream_config,
                channels,
                position_bits,
                resync_generation,
                decoded,
            ),
            other => return Err(format!("unsupported audio output sample format: {other}")),
        }
        .map_err(|err| format!("failed to build audio output stream: {err}"))?;

        // Deliberately not `.play()`'d here — a freshly built `cpal::Stream` doesn't produce
        // any callbacks until started, so leaving it alone means audio stays silent until the
        // caller's next `set_playing(true)`, matching `load_video` always resetting
        // `ui_state.playing` to `false`. Calling `.play()` here would start audio immediately on
        // load regardless of the (always-paused-after-load) UI state.
        self.stream = Some(stream);
        Ok(())
    }

    /// Mirrors the transport position into the audio callback. This is called at redraw cadence,
    /// so the callback treats it as an anchor for scrubs/resyncs rather than restarting every
    /// output buffer from this exact position.
    pub fn set_position_seconds(&self, seconds: f64) {
        self.position_bits
            .store(seconds.max(0.0).to_bits(), Ordering::Relaxed);
    }

    /// Moves the audio cursor to an explicit transport seek target. Use this for user-driven
    /// scrubs/seeks, not ordinary playback ticks.
    pub fn seek_to_position_seconds(&self, seconds: f64) {
        self.set_position_seconds(seconds);
        self.resync_generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Pauses/resumes the underlying stream (a no-op if nothing is loaded, or if the device is
    /// already in the requested state — `cpal::Stream::play`/`pause` are themselves idempotent).
    pub fn set_playing(&self, playing: bool) {
        let Some(stream) = self.stream.as_ref() else {
            return;
        };
        let result = if playing {
            stream.play()
        } else {
            stream.pause()
        };
        if let Err(err) = result {
            let action = if playing { "resume" } else { "pause" };
            eprintln!("failed to {action} audio stream: {err}");
        }
    }
}

/// Whole decoded track, resampled to stereo f32 at the output device's own sample rate.
struct DecodedAudio {
    left: Vec<f32>,
    right: Vec<f32>,
    sample_rate: u32,
}

/// Same decode/resample shape as `crates/export/src/audio.rs::decode_all` (a second,
/// independent `ffmpeg_next::format::input` open, not shared with `video-pipeline`'s — see that
/// module's doc comment for why), duplicated rather than imported for the same reason
/// `has_audio_stream` is above.
fn decode_all(path: &Path, target_sample_rate: i32) -> Result<DecodedAudio, String> {
    let mut input = ffmpeg::format::input(path)
        .map_err(|err| format!("failed to open {path:?} for audio decode: {err}"))?;

    let Some(stream) = input.streams().best(ffmpeg::media::Type::Audio) else {
        return Ok(DecodedAudio {
            left: Vec::new(),
            right: Vec::new(),
            sample_rate: target_sample_rate as u32,
        });
    };
    let stream_index = stream.index();

    let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .map_err(|err| format!("failed to build audio decoder context: {err}"))?;
    let mut decoder = context
        .decoder()
        .audio()
        .map_err(|err| format!("failed to open audio decoder: {err}"))?;

    // On Windows static FFmpeg 7.x builds, AVCodecContext.ch_layout.nb_channels can be zeroed
    // even though the stream parameters are correct — guard against that so the resampler init
    // and plane-count derivation below don't produce garbage.
    let src_layout = {
        let l = decoder.channel_layout();
        if l.channels() <= 0 {
            ffmpeg::ChannelLayout::STEREO
        } else {
            l
        }
    };

    let mut resampler = ResamplingContext::get(
        decoder.format(),
        src_layout,
        decoder.rate(),
        Sample::F32(Type::Planar),
        ffmpeg::ChannelLayout::STEREO,
        target_sample_rate as u32,
    )
    .map_err(|err| format!("failed to build audio resampler: {err}"))?;

    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut raw = AudioFrame::empty();

    // For planar formats each channel has its own data[] plane; for packed/interleaved formats
    // all channels share data[0]. Using channel_layout.channels() unconditionally was wrong
    // for packed formats where data[1] is null, causing wrong output or memory errors.
    let n_in_planes = if decoder.format().is_planar() {
        (src_layout.channels() as usize).max(1)
    } else {
        1
    };

    let drain_decoder = |decoder: &mut ffmpeg::decoder::Audio,
                         resampler: &mut ResamplingContext,
                         raw: &mut AudioFrame,
                         left: &mut Vec<f32>,
                         right: &mut Vec<f32>|
     -> Result<(), String> {
        while decoder.receive_frame(raw).is_ok() {
            // Use swr_convert (not swr_convert_frame) to bypass the frame-metadata
            // consistency check — on Windows static FFmpeg 7.x builds, decoded AAC
            // frames leave ch_layout and sample_rate zeroed even though the PCM data
            // itself is valid, and swr_convert_frame rejects them with AVERROR_*_CHANGED.
            let in_samples = raw.samples() as i32;
            let out_size =
                (resampler.out_sample_count(in_samples) as usize).max(in_samples as usize + 256);
            let mut out_left = vec![0f32; out_size];
            let mut out_right = vec![0f32; out_size];
            // Access data[] directly — plane() uses ch_layout.nb_channels which is
            // zeroed on Windows; data[] is always populated by the decoder.
            let in_planes: Vec<*const u8> = (0..n_in_planes)
                .map(|i| unsafe { (*raw.as_ptr()).data[i] as *const u8 })
                .collect();
            let n = resampler
                .convert_planes(&in_planes, in_samples, &mut out_left, &mut out_right)
                .map_err(|err| format!("audio resample error: {err}"))?;
            left.extend_from_slice(&out_left[..n]);
            right.extend_from_slice(&out_right[..n]);
        }
        Ok(())
    };

    for (stream, packet) in input.packets() {
        if stream.index() != stream_index {
            continue;
        }
        decoder
            .send_packet(&packet)
            .map_err(|err| format!("audio decode error: {err}"))?;
        drain_decoder(
            &mut decoder,
            &mut resampler,
            &mut raw,
            &mut left,
            &mut right,
        )?;
    }
    decoder.send_eof().ok();
    drain_decoder(
        &mut decoder,
        &mut resampler,
        &mut raw,
        &mut left,
        &mut right,
    )?;

    // Drain any samples swresample is still holding onto internally after the last real frame.
    loop {
        let mut fl = vec![0f32; 4096];
        let mut fr = vec![0f32; 4096];
        let n = resampler
            .convert_planes(&[], 0, &mut fl, &mut fr)
            .unwrap_or(0);
        if n == 0 {
            break;
        }
        left.extend_from_slice(&fl[..n]);
        right.extend_from_slice(&fr[..n]);
    }

    Ok(DecodedAudio {
        left,
        right,
        sample_rate: target_sample_rate as u32,
    })
}

/// Downsamples the decoded stereo track into one peak-amplitude value per
/// `WAVEFORM_BUCKET_SECONDS`, taking the louder of the two channels per sample — cheap to draw
/// from at any timeline zoom level since the UI only ever re-buckets this already-small array,
/// never the raw sample data.
fn compute_waveform_peaks(left: &[f32], right: &[f32], sample_rate: u32) -> Vec<f32> {
    let bucket_samples = ((sample_rate as f64 * WAVEFORM_BUCKET_SECONDS) as usize).max(1);
    let len = left.len().min(right.len());
    let mut peaks = Vec::with_capacity(len / bucket_samples + 1);
    let mut start = 0;
    while start < len {
        let end = (start + bucket_samples).min(len);
        let peak = left[start..end]
            .iter()
            .zip(&right[start..end])
            .fold(0.0f32, |acc, (&l, &r)| acc.max(l.abs()).max(r.abs()));
        peaks.push(peak);
        start = end;
    }
    peaks
}

fn build_typed_stream<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    stream_config: cpal::StreamConfig,
    channels: usize,
    position_bits: Arc<AtomicU64>,
    resync_generation: Arc<AtomicU64>,
    decoded: DecodedAudio,
) -> Result<cpal::Stream, cpal::Error> {
    let err_fn = |err| eprintln!("audio output stream error: {err}");
    let DecodedAudio {
        left,
        right,
        sample_rate,
    } = decoded;
    let mut cursor = PlaybackCursor::new(sample_rate);

    device.build_output_stream(
        stream_config,
        move |output: &mut [T], _: &cpal::OutputCallbackInfo| {
            fill_buffer(
                output,
                channels,
                &position_bits,
                &resync_generation,
                &mut cursor,
                &left,
                &right,
            );
        },
        err_fn,
        None,
    )
}

struct PlaybackCursor {
    sample_rate: u32,
    next_index: usize,
    resync_threshold_samples: usize,
    last_resync_generation: u64,
    initialized: bool,
}

impl PlaybackCursor {
    fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            next_index: 0,
            resync_threshold_samples: (RESYNC_THRESHOLD_SECONDS * sample_rate as f64) as usize,
            last_resync_generation: 0,
            initialized: false,
        }
    }

    fn anchor_index(&self, position_bits: &AtomicU64) -> usize {
        let position_seconds = f64::from_bits(position_bits.load(Ordering::Relaxed));
        (position_seconds.max(0.0) * self.sample_rate as f64) as usize
    }

    fn start_index(&mut self, position_bits: &AtomicU64, resync_generation: &AtomicU64) -> usize {
        let anchor_index = self.anchor_index(position_bits);
        let generation = resync_generation.load(Ordering::Relaxed);
        let explicit_resync = generation != self.last_resync_generation;
        // Only resync when audio is lagging BEHIND the video anchor — being ahead is normal
        // (the callback fills a whole buffer at once, leaving cursor buffer-size ahead of the
        // anchor until the next video-position update). Resyncing on "ahead" as well with
        // abs_diff meant that any audio device with a buffer larger than the threshold
        // (50 ms) would resync on every callback, making audio stutter continuously.
        let lagging = anchor_index.saturating_sub(self.next_index) > self.resync_threshold_samples;
        let should_resync = !self.initialized || explicit_resync || lagging;

        if should_resync {
            self.next_index = anchor_index;
            self.last_resync_generation = generation;
            self.initialized = true;
        }

        self.next_index
    }
}

/// Fills one output callback's worth of samples starting at whatever sample index
/// the playback cursor currently maps to, using `position_bits` only to detect scrubs and larger
/// drift. Interleaves to however many channels the device actually wants, duplicating L/R into
/// any channels beyond the first two (mirrors Neothesia's own `SynthBackend::run`, which faces
/// the identical "device may not be exactly stereo" problem).
fn fill_buffer<T: cpal::Sample + cpal::FromSample<f32>>(
    output: &mut [T],
    channels: usize,
    position_bits: &AtomicU64,
    resync_generation: &AtomicU64,
    cursor: &mut PlaybackCursor,
    left: &[f32],
    right: &[f32],
) {
    let start_index = cursor.start_index(position_bits, resync_generation);

    for (i, frame) in output.chunks_mut(channels).enumerate() {
        let index = start_index + i;
        let (l, r) = if index < left.len() {
            (left[index], right[index])
        } else {
            (0.0, 0.0)
        };
        let samples = [T::from_sample(l), T::from_sample(r)];
        for (channel, sample) in frame.iter_mut().enumerate() {
            *sample = samples[channel % 2];
        }
    }
    cursor.next_index = start_index + output.len() / channels;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_buffer_advances_between_redraw_position_updates() {
        let position_bits = AtomicU64::new(0.0_f64.to_bits());
        let resync_generation = AtomicU64::new(0);
        let mut cursor = PlaybackCursor::new(100);
        let left: Vec<f32> = (0..20).map(|value| value as f32).collect();
        let right: Vec<f32> = (100..120).map(|value| value as f32).collect();
        let mut output = vec![0.0_f32; 8];

        fill_buffer(
            &mut output,
            2,
            &position_bits,
            &resync_generation,
            &mut cursor,
            &left,
            &right,
        );
        assert_eq!(output, [0.0, 100.0, 1.0, 101.0, 2.0, 102.0, 3.0, 103.0]);

        fill_buffer(
            &mut output,
            2,
            &position_bits,
            &resync_generation,
            &mut cursor,
            &left,
            &right,
        );
        assert_eq!(output, [4.0, 104.0, 5.0, 105.0, 6.0, 106.0, 7.0, 107.0]);
    }

    #[test]
    fn fill_buffer_resyncs_after_large_position_jump() {
        let position_bits = AtomicU64::new(0.0_f64.to_bits());
        let resync_generation = AtomicU64::new(0);
        let mut cursor = PlaybackCursor::new(100);
        let left: Vec<f32> = (0..40).map(|value| value as f32).collect();
        let right: Vec<f32> = (100..140).map(|value| value as f32).collect();
        let mut output = vec![0.0_f32; 4];

        fill_buffer(
            &mut output,
            2,
            &position_bits,
            &resync_generation,
            &mut cursor,
            &left,
            &right,
        );

        position_bits.store(0.20_f64.to_bits(), Ordering::Relaxed);
        fill_buffer(
            &mut output,
            2,
            &position_bits,
            &resync_generation,
            &mut cursor,
            &left,
            &right,
        );

        assert_eq!(output, [20.0, 120.0, 21.0, 121.0]);
    }

    #[test]
    fn fill_buffer_resyncs_after_explicit_tiny_seek() {
        let position_bits = AtomicU64::new(0.0_f64.to_bits());
        let resync_generation = AtomicU64::new(0);
        let mut cursor = PlaybackCursor::new(100);
        let left: Vec<f32> = (0..40).map(|value| value as f32).collect();
        let right: Vec<f32> = (100..140).map(|value| value as f32).collect();
        let mut output = vec![0.0_f32; 4];

        fill_buffer(
            &mut output,
            2,
            &position_bits,
            &resync_generation,
            &mut cursor,
            &left,
            &right,
        );

        position_bits.store(0.03_f64.to_bits(), Ordering::Relaxed);
        resync_generation.fetch_add(1, Ordering::Relaxed);
        fill_buffer(
            &mut output,
            2,
            &position_bits,
            &resync_generation,
            &mut cursor,
            &left,
            &right,
        );

        assert_eq!(output, [3.0, 103.0, 4.0, 104.0]);
    }
}
