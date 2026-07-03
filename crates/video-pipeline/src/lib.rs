use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{context::Context as ScalingContext, flag::Flags};
use ffmpeg_next::util::frame::video::Video as VideoFrame;

/// One decoded video frame, ready to upload to a GPU texture.
#[derive(Clone)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    /// Tightly packed BGRA8, row-major, no stride padding.
    pub bgra: Vec<u8>,
    /// Presentation timestamp in seconds, relative to stream start.
    pub pts_seconds: f64,
}

/// Forward steps larger than this are treated as a scrub jump (triggers a real seek) rather
/// than ordinary playback advancing frame-by-frame.
const MAX_FORWARD_STEP_SECONDS: f64 = 1.0;

pub struct VideoPipeline {
    input: ffmpeg::format::context::Input,
    stream_index: usize,
    decoder: ffmpeg::codec::decoder::Video,
    scaler: ScalingContext,
    time_base: ffmpeg::Rational,
    duration_seconds: f64,
    pub width: u32,
    pub height: u32,
    /// Most recently decoded frame, held so callers advancing by a sub-frame-duration step
    /// (typical between two redraws) can be served without touching the decoder at all.
    current_frame: Option<DecodedFrame>,
}

impl VideoPipeline {
    pub fn open(path: &std::path::Path) -> Result<Self, ffmpeg::Error> {
        ffmpeg::init()?;

        let input = ffmpeg::format::input(path)?;
        let stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?;
        let stream_index = stream.index();
        let time_base = stream.time_base();
        let duration_seconds = if stream.duration() > 0 {
            stream.duration() as f64 * f64::from(time_base)
        } else {
            input.duration() as f64 / f64::from(ffmpeg::rescale::TIME_BASE)
        };

        let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = context.decoder().video()?;

        let width = decoder.width();
        let height = decoder.height();

        let scaler = ScalingContext::get(
            decoder.format(),
            width,
            height,
            Pixel::BGRA,
            width,
            height,
            Flags::BILINEAR,
        )?;

        Ok(Self {
            input,
            stream_index,
            decoder,
            scaler,
            time_base,
            duration_seconds,
            width,
            height,
            current_frame: None,
        })
    }

    pub fn duration_seconds(&self) -> f64 {
        self.duration_seconds
    }

    /// Returns the frame that should be displayed at `target_seconds`.
    ///
    /// Ordinary playback calls this every redraw with `target_seconds` advancing by a few
    /// milliseconds at a time; in that case no seek happens at all, and if the currently held
    /// frame is still the correct one to display, the decoder isn't touched either — only a
    /// backward jump or a forward jump bigger than [`MAX_FORWARD_STEP_SECONDS`] (a scrub) causes
    /// a real seek.
    ///
    /// `exact = false` (scrubbing/preview) returns the first frame decoded after a seek, which
    /// lands on/near the nearest preceding keyframe — cheap but approximate. `exact = true`
    /// (export) keeps decoding forward until the frame's timestamp reaches `target_seconds`, so
    /// the result is frame-accurate.
    pub fn seek_and_decode(
        &mut self,
        target_seconds: f64,
        exact: bool,
    ) -> Result<DecodedFrame, ffmpeg::Error> {
        let need_seek = match self.current_frame.as_ref() {
            None => true,
            Some(frame) => {
                target_seconds + 1e-6 < frame.pts_seconds
                    || target_seconds - frame.pts_seconds > MAX_FORWARD_STEP_SECONDS
            }
        };

        if need_seek {
            // `Input::seek` calls `avformat_seek_file` with `stream_index = -1`, which means
            // the timestamp is in `AV_TIME_BASE` (microsecond) units, not this stream's own
            // time base.
            let target_ts = (target_seconds / f64::from(ffmpeg::rescale::TIME_BASE)) as i64;
            self.input.seek(target_ts, ..target_ts)?;
            self.decoder.flush();
        } else if !exact {
            if let Some(frame) = self.current_frame.as_ref() {
                if target_seconds <= frame.pts_seconds {
                    return Ok(frame.clone());
                }
            }
        }

        for (stream, packet) in self.input.packets() {
            if stream.index() != self.stream_index {
                continue;
            }
            self.decoder.send_packet(&packet)?;
            let mut raw = VideoFrame::empty();
            while self.decoder.receive_frame(&mut raw).is_ok() {
                let pts_seconds = raw
                    .pts()
                    .map(|pts| pts as f64 * f64::from(self.time_base))
                    .unwrap_or(target_seconds);

                let mut scaled = VideoFrame::empty();
                self.scaler.run(&raw, &mut scaled)?;
                let frame = to_decoded_frame(&scaled, pts_seconds);
                self.current_frame = Some(frame.clone());

                if !exact || pts_seconds >= target_seconds {
                    return Ok(frame);
                }
            }
        }

        self.current_frame.clone().ok_or(ffmpeg::Error::Eof)
    }
}

fn to_decoded_frame(scaled: &VideoFrame, pts_seconds: f64) -> DecodedFrame {
    let width = scaled.width();
    let height = scaled.height();
    let stride = scaled.stride(0);
    let data = scaled.data(0);
    let row_bytes = width as usize * 4;

    let mut bgra = Vec::with_capacity(row_bytes * height as usize);
    for row in 0..height as usize {
        let start = row * stride;
        bgra.extend_from_slice(&data[start..start + row_bytes]);
    }

    DecodedFrame {
        width,
        height,
        bgra,
        pts_seconds,
    }
}
