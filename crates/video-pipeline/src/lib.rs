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

/// Result of a decode request that borrows the pipeline's cached frame.
pub struct DecodedFrameRef<'a> {
    pub frame: &'a DecodedFrame,
    /// True when this call decoded a different source frame. False means the cached frame already
    /// covered the requested timestamp and callers can skip texture uploads.
    pub changed: bool,
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
    frame_duration_seconds: f64,
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
        let frame_rate = stream.avg_frame_rate();
        let frame_duration_seconds = if frame_rate.numerator() > 0 && frame_rate.denominator() > 0 {
            frame_rate.denominator() as f64 / frame_rate.numerator() as f64
        } else {
            1.0 / 30.0
        };

        let mut context = ffmpeg::codec::context::Context::from_parameters(stream.parameters())?;
        // Frame-level multithreading: `avcodec_open2` (called inside `.video()` below) reads
        // `thread_count`/`thread_type` at open time, and leaving them unset defaults to
        // single-threaded decode. That's fine for `scripts/gen-test-video.sh`'s small synthetic
        // clips but not for real footage — a 1920x1080 H.264 clip measured ~4.7ms/frame average
        // (p95 7ms, spikes over 20ms) decoding on one thread, enough to blow well past a single
        // redraw's budget and stutter during playback. `Type::Frame` (not `Type::Slice`) because
        // consumer camera encoders (the case this app targets) typically write one slice per
        // frame, so slice-threading has nothing to parallelize; frame-threading decodes multiple
        // frames concurrently instead, at the cost of a few frames of decode latency — irrelevant
        // for a scrub/playback pipeline that's never decoding "live". `count: 0` lets libavcodec
        // pick the thread count itself (capped by the stream's own reference-frame count/level).
        context.set_threading(ffmpeg::threading::Config {
            kind: ffmpeg::threading::Type::Frame,
            count: 0,
        });
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
            frame_duration_seconds,
            width,
            height,
            current_frame: None,
        })
    }

    pub fn duration_seconds(&self) -> f64 {
        self.duration_seconds
    }

    pub fn frame_duration_seconds(&self) -> f64 {
        self.frame_duration_seconds
    }

    /// Returns the frame that should be displayed at `target_seconds`.
    ///
    /// Ordinary playback calls this every redraw with `target_seconds` advancing by a few
    /// milliseconds at a time; in that case no seek happens at all, and if the currently held
    /// frame is still the correct one to display, the decoder isn't touched either. Only a
    /// backward jump or a forward jump bigger than [`MAX_FORWARD_STEP_SECONDS`] causes a real
    /// seek — this is meant to catch actual scrubs, but it also fires for something that isn't a
    /// scrub at all: a redraw-cadence stall (a slow frame from system load, a blocked GPU call,
    /// anything that makes real wall-clock time jump by more than a second between redraws while
    /// `target_seconds` was simply advancing via elapsed time, not an explicit seek).
    ///
    /// `exact` controls what happens *after* one of those reseeks, and callers should set it
    /// based on which of those two cases they're actually in — this distinction used to not
    /// exist, and conflating them was a real bug (reported as "playback jumps back and gets
    /// stuck looping ~20 frames" once redraw cadence on real hardware turned out to be less
    /// metronomic than initially assumed): `exact = false` (an explicit scrub — the interactive
    /// timeline being dragged/clicked) returns the first frame decoded after the seek, which
    /// lands on/near the nearest preceding keyframe — cheap but approximate, and fine for that
    /// case since the user is actively dragging and will settle on a final position themselves.
    /// `exact = true` (export, and — critically — ordinary playback ticks too, even though those
    /// don't otherwise look like they need "exactness") keeps decoding forward until the frame's
    /// timestamp reaches `target_seconds`. For ordinary playback this matters specifically for
    /// the stall case: without it, a stall-triggered reseek would land on a stale, up-to-2-second
    /// old keyframe and just sit there — every following redraw would see the same
    /// still-uncaught-up gap, trigger *another* reseek to essentially the same spot, and repeat,
    /// which looks exactly like the video jumping back and stuttering through the same handful
    /// of frames instead of smoothly continuing forward.
    pub fn seek_and_decode(
        &mut self,
        target_seconds: f64,
        exact: bool,
    ) -> Result<DecodedFrame, ffmpeg::Error> {
        self.seek_and_decode_ref(target_seconds, exact)
            .map(|decoded| decoded.frame.clone())
    }

    pub fn seek_and_decode_ref(
        &mut self,
        target_seconds: f64,
        exact: bool,
    ) -> Result<DecodedFrameRef<'_>, ffmpeg::Error> {
        self.decode_ref(target_seconds, exact)
    }

    fn decode_ref(
        &mut self,
        target_seconds: f64,
        exact: bool,
    ) -> Result<DecodedFrameRef<'_>, ffmpeg::Error> {
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
        } else if self
            .current_frame
            .as_ref()
            .is_some_and(|frame| target_seconds < frame.pts_seconds + self.frame_duration_seconds)
        {
            // Already covered by the held frame — nothing to decode. This shortcut used to live
            // behind `!exact` (i.e. only ordinary playback ticks got it, never a caller passing
            // `exact = true`), but it's unconditionally safe: it only returns early when the
            // held frame already satisfies `target_seconds`, which `exact` doesn't change the
            // meaning of. Gating it behind `!exact` had no effect on `export` (which always
            // requests strictly increasing targets, so the held frame is never already ahead)
            // but mattered once interactive playback started passing `exact = true` too (see
            // below) — without this, every redraw would force a fresh decode even when the
            // currently displayed frame was still perfectly valid, subtly speeding up playback
            // on any redraw cadence faster than the video's own frame rate.
            return Ok(DecodedFrameRef {
                frame: self
                    .current_frame
                    .as_ref()
                    .expect("cached frame was just checked"),
                changed: false,
            });
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

                // Only scale+copy the frame we're actually about to hand back. During a
                // catch-up burst (`exact = true` and several frames between the last held one
                // and `target_seconds`), every earlier frame in the burst used to still pay for
                // a full-frame swscale conversion plus an ~8MB `Vec` allocation/copy in
                // `to_decoded_frame`, immediately thrown away once the next `receive_frame` came
                // back — real, measured cost (not skipped work) for frames nobody ever saw.
                if exact && pts_seconds < target_seconds {
                    continue;
                }

                let mut scaled = VideoFrame::empty();
                self.scaler.run(&raw, &mut scaled)?;
                let frame = to_decoded_frame(&scaled, pts_seconds);
                self.current_frame = Some(frame);
                return Ok(DecodedFrameRef {
                    frame: self.current_frame.as_ref().expect("frame was just cached"),
                    changed: true,
                });
            }
        }

        self.current_frame
            .as_ref()
            .map(|frame| DecodedFrameRef {
                frame,
                changed: false,
            })
            .ok_or(ffmpeg::Error::Eof)
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
