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

pub struct AudioPlayback {
    stream: Option<cpal::Stream>,
    /// Transport position in seconds (as `f64::to_bits`), updated once per redraw by
    /// `set_position_seconds`. The output callback re-reads this fresh on every invocation
    /// rather than advancing its own internal sample counter between calls — the position can
    /// only ever come from the (video-driven) transport, so audio can't accumulate drift.
    position_bits: Arc<AtomicU64>,
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
        }
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
        let channels = config.channels() as usize;
        let stream_config: cpal::StreamConfig = config.into();
        let position_bits = self.position_bits.clone();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                build_typed_stream::<f32>(&device, stream_config, channels, position_bits, decoded)
            }
            cpal::SampleFormat::I16 => {
                build_typed_stream::<i16>(&device, stream_config, channels, position_bits, decoded)
            }
            cpal::SampleFormat::U16 => {
                build_typed_stream::<u16>(&device, stream_config, channels, position_bits, decoded)
            }
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

    /// Mirrors the transport position into the audio callback — called once per redraw, the
    /// same cadence video decode and the MIDI overlay are already driven at. `seconds` is
    /// expected to already be clamped by the caller (matches `ui_state.position_seconds`).
    pub fn set_position_seconds(&self, seconds: f64) {
        self.position_bits
            .store(seconds.max(0.0).to_bits(), Ordering::Relaxed);
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

    let mut resampler = ResamplingContext::get(
        decoder.format(),
        decoder.channel_layout(),
        decoder.rate(),
        Sample::F32(Type::Planar),
        ffmpeg::ChannelLayout::STEREO,
        target_sample_rate as u32,
    )
    .map_err(|err| format!("failed to build audio resampler: {err}"))?;

    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut raw = AudioFrame::empty();

    let drain_decoder = |decoder: &mut ffmpeg::decoder::Audio,
                         resampler: &mut ResamplingContext,
                         raw: &mut AudioFrame,
                         left: &mut Vec<f32>,
                         right: &mut Vec<f32>|
     -> Result<(), String> {
        while decoder.receive_frame(raw).is_ok() {
            let mut resampled = AudioFrame::empty();
            resampler
                .run(raw, &mut resampled)
                .map_err(|err| format!("audio resample error: {err}"))?;
            left.extend_from_slice(resampled.plane::<f32>(0));
            right.extend_from_slice(resampled.plane::<f32>(1));
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
        let mut resampled = AudioFrame::empty();
        match resampler.flush(&mut resampled) {
            Ok(Some(_)) if resampled.samples() > 0 => {
                left.extend_from_slice(resampled.plane::<f32>(0));
                right.extend_from_slice(resampled.plane::<f32>(1));
            }
            _ => break,
        }
    }

    Ok(DecodedAudio {
        left,
        right,
        sample_rate: target_sample_rate as u32,
    })
}

fn build_typed_stream<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    stream_config: cpal::StreamConfig,
    channels: usize,
    position_bits: Arc<AtomicU64>,
    decoded: DecodedAudio,
) -> Result<cpal::Stream, cpal::Error> {
    let err_fn = |err| eprintln!("audio output stream error: {err}");
    let DecodedAudio {
        left,
        right,
        sample_rate,
    } = decoded;

    device.build_output_stream(
        stream_config,
        move |output: &mut [T], _: &cpal::OutputCallbackInfo| {
            fill_buffer(output, channels, &position_bits, sample_rate, &left, &right);
        },
        err_fn,
        None,
    )
}

/// Fills one output callback's worth of samples starting at whatever sample index
/// `position_bits` currently maps to, silence past the end of the decoded track (or before its
/// start). Interleaves to however many channels the device actually wants, duplicating L/R into
/// any channels beyond the first two (mirrors Neothesia's own `SynthBackend::run`, which faces
/// the identical "device may not be exactly stereo" problem).
fn fill_buffer<T: cpal::Sample + cpal::FromSample<f32>>(
    output: &mut [T],
    channels: usize,
    position_bits: &AtomicU64,
    sample_rate: u32,
    left: &[f32],
    right: &[f32],
) {
    let position_seconds = f64::from_bits(position_bits.load(Ordering::Relaxed));
    let start_index = (position_seconds * sample_rate as f64) as usize;

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
}
