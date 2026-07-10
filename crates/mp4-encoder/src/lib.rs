//! Forked from https://github.com/PolyMeilex/Neothesia (`ffmpeg-encoder` crate) at commit
//! e61639b12cc8e466b90406c564da5f9f54d8d1a3, under GPL-3.0 (see LICENSE) — this whole app is
//! already GPL-3.0-only, so relicensing at the fork point isn't an issue. See
//! `docs/architecture.md`'s mp4-encoder section for the deltas from upstream.
//!
//! Based on: https://ffmpeg.org/doxygen/trunk/mux_8c-example.html

use std::{ffi::CString, path::Path};

use ffmpeg::{AVPixelFormat, AVERROR, AVERROR_EOF, EAGAIN};

mod audio;
mod ff;
mod video;

const STREAM_PIX_FMT: AVPixelFormat = AVPixelFormat::AV_PIX_FMT_YUV420P;
const SRC_STREAM_PIX_FMT: AVPixelFormat = AVPixelFormat::AV_PIX_FMT_BGRA;

/// Encode one frame and send it to the muxer.
/// Returns true when encoding is finished, false otherwise.
fn write_frame(
    codec_ctx: &ff::CodecContext,
    stream: &ff::Stream,
    packet: &ff::Packet,
    format_ctx: &ff::FormatContext,
    frame: Option<&ff::Frame>,
) -> bool {
    // Send the frame to the encoder
    codec_ctx.send_frame(frame);

    let mut ret = 0;
    while ret >= 0 {
        ret = codec_ctx.receive_packet(packet);

        if ret == AVERROR(EAGAIN) || ret == AVERROR_EOF {
            break;
        } else if ret < 0 {
            panic!("Error encoding a frame",);
        }

        // Rescale output packet timestamp values from codec to stream timebase
        packet.rescale_ts(codec_ctx.time_base(), stream.time_base());
        packet.set_stream_index(stream.index());

        // Write the compressed frame to the media file.
        format_ctx.interleaved_write_frame(packet);
    }

    ret == AVERROR_EOF
}

pub enum Frame<'a> {
    Vide(&'a [u8]),
    Audio(&'a [f32], &'a [f32]),
    Terminator,
}

#[derive(Debug)]
pub struct EncoderInfo {
    pub frame_size: usize,
    /// The audio codec's actual chosen sample rate (it isn't necessarily whatever was
    /// requested internally — see `audio.rs`), so callers can target their own resampler
    /// correctly. `0` when `with_audio` was `false`.
    pub sample_rate: i32,
}

/// `fps` sets both the output timebase and (via `video::VideoOutputStream::new`) the keyframe
/// interval. `with_audio` controls whether an audio stream/codec is created at all — pass
/// `false` for a source with no audio track rather than encoding silence.
pub fn new(
    path: impl AsRef<Path>,
    width: u32,
    height: u32,
    fps: u32,
    with_audio: bool,
) -> (EncoderInfo, impl FnMut(Frame<'_>)) {
    let path = path.as_ref().to_str().unwrap();
    let path = CString::new(path).unwrap();

    let format_context = ff::FormatContext::new(&path);

    let output_format = format_context.output_format();

    let video_stream = video::VideoOutputStream::new(
        &format_context,
        &output_format,
        width as i32,
        height as i32,
        fps,
    );
    let audio_stream =
        with_audio.then(|| audio::new_audio_streams(&format_context, &output_format));

    format_context.dump_format(&path);
    format_context.open(&path);
    // Write the stream header, if any.
    format_context.write_header();

    let frame_size = audio_stream
        .as_ref()
        .map(|a| a.codec_ctx.frame_size() as usize)
        .unwrap_or(0);
    let sample_rate = audio_stream
        .as_ref()
        .map(|a| a.codec_ctx.sample_rate())
        .unwrap_or(0);
    let info = EncoderInfo {
        frame_size,
        sample_rate,
    };

    let mut ctx = Some((video_stream, audio_stream, format_context));

    (info, move |input_frame| match input_frame {
        Frame::Vide(input_frame) => {
            let (video_stream, _audio_stream, format_context) =
                ctx.as_mut().expect("Encoder should not be closed");
            video_stream.write_frame(format_context, input_frame);
        }
        Frame::Audio(l, r) => {
            let (_video_stream, audio_stream, format_context) =
                ctx.as_mut().expect("Encoder should not be closed");
            if let Some(audio_stream) = audio_stream.as_mut() {
                audio_stream.write_frame(format_context, l, r);
            }
        }
        Frame::Terminator => {
            let (video_stream, audio_stream, format_ctx) =
                ctx.take().expect("Encoder should not be closed");

            video_stream.write_terminator_frame(&format_ctx);
            if let Some(audio_stream) = audio_stream {
                audio_stream.write_terminator_frame(&format_ctx);
            }

            format_ctx.write_trailer();
        }
    })
}
