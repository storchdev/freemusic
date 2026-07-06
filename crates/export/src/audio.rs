//! Decodes the source video's own audio track (if it has one) fully upfront, resampled to
//! stereo f32 at the encoder's actual chosen sample rate. A second, independent
//! `ffmpeg_next::format::input` open rather than sharing `video-pipeline`'s — that pipeline's
//! `Input` is driven by exact/inexact-seek video decode state and isn't set up to interleave
//! audio packet extraction, and re-opening the file is cheap next to the cost of the export
//! render loop itself.

use std::path::Path;

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::sample::{Sample, Type};
use ffmpeg_next::software::resampling::context::Context as ResamplingContext;
use ffmpeg_next::util::frame::audio::Audio as AudioFrame;

pub struct DecodedAudio {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

/// Cheap probe so the caller can decide `with_audio` for `mp4_encoder::new` *before* paying for
/// a full decode — the encoder's resulting `sample_rate` is then fed back into `decode_all` as
/// the resample target, avoiding a chicken-and-egg between "does audio exist" and "what rate
/// should it be resampled to."
pub fn has_audio_stream(path: &Path) -> bool {
    ffmpeg::format::input(path)
        .ok()
        .and_then(|input| input.streams().best(ffmpeg::media::Type::Audio).map(|_| ()))
        .is_some()
}

pub fn decode_all(path: &Path, target_sample_rate: i32) -> Result<DecodedAudio, String> {
    let mut input = ffmpeg::format::input(path)
        .map_err(|err| format!("failed to open {path:?} for audio decode: {err}"))?;

    let Some(stream) = input.streams().best(ffmpeg::media::Type::Audio) else {
        return Ok(DecodedAudio {
            left: Vec::new(),
            right: Vec::new(),
        });
    };
    let stream_index = stream.index();

    let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .map_err(|err| format!("failed to build audio decoder context: {err}"))?;
    let mut decoder = context
        .decoder()
        .audio()
        .map_err(|err| format!("failed to open audio decoder: {err}"))?;

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
            // frames leave ch_layout and sample_rate zeroed, causing AVERROR_*_CHANGED.
            let in_samples = raw.samples() as i32;
            let out_size =
                (resampler.out_sample_count(in_samples) as usize).max(in_samples as usize + 256);
            let mut out_left = vec![0f32; out_size];
            let mut out_right = vec![0f32; out_size];
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

    Ok(DecodedAudio { left, right })
}
