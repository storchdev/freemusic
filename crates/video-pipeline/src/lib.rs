use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::color_space::ColorSpace as SwsColorSpace;
use ffmpeg_next::software::scaling::{context::Context as ScalingContext, flag::Flags};
use ffmpeg_next::util::color::{Range as ColorRange, Space as ColorSpace};
use ffmpeg_next::util::frame::video::Video as VideoFrame;

/// Maps a stream's tagged colorspace to the `SWS_CS_*` matrix swscale should use, with a
/// resolution-based fallback for `Unspecified` (the common case — most containers don't bother
/// tagging this) matching the convention most other players/tools use: SD content defaults to
/// BT.601, HD/UHD defaults to BT.709.
fn sws_colorspace(space: ColorSpace, height: u32) -> SwsColorSpace {
    match space {
        ColorSpace::BT709 => SwsColorSpace::ITU709,
        ColorSpace::BT470BG => SwsColorSpace::ITU601,
        ColorSpace::SMPTE170M => SwsColorSpace::SMPTE170M,
        ColorSpace::SMPTE240M => SwsColorSpace::SMPTE240M,
        ColorSpace::FCC => SwsColorSpace::FCC,
        _ => {
            if height >= 720 {
                SwsColorSpace::ITU709
            } else {
                SwsColorSpace::ITU601
            }
        }
    }
}

/// `ScalingContext::get`/`sws_getContext` only take pixel format and size — no colorspace or
/// color-range info — so swscale silently falls back to its own default (BT.601, limited/MPEG
/// range in and out). Camera footage is very commonly BT.709 and/or full-range, and without this
/// call the resulting BGRA came out visibly darker/washed-out than reference players (mpv, iOS
/// Photos/Camera) that read and correct for the stream's actual color metadata. `dstRange` is
/// always full (1) since the destination is BGRA/RGB, which has no separate "limited" encoding.
fn apply_colorspace_details(
    scaler: &mut ScalingContext,
    color_space: ColorSpace,
    color_range: ColorRange,
    height: u32,
) {
    let sws_cs: std::os::raw::c_int = sws_colorspace(color_space, height).into();
    let src_full_range = matches!(color_range, ColorRange::JPEG);
    unsafe {
        let coeffs = ffmpeg::ffi::sws_getCoefficients(sws_cs);
        ffmpeg::ffi::sws_setColorspaceDetails(
            scaler.as_mut_ptr(),
            coeffs,
            src_full_range as std::os::raw::c_int,
            coeffs,
            1,
            0,
            1 << 16,
            1 << 16,
        );
    }
}

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
    /// Sub-stage timings for the most recent `decode_ref` call, so the app's perf logging can
    /// tell *which* purely-CPU stage of a decode balloons under load (h264 decode vs swscale
    /// colour-convert vs the readback copy) instead of only seeing one aggregate number. Reset
    /// at the top of every `decode_ref` call. None of these stages issue a GPU call, so if any
    /// of them balloons the cause is CPU/IO contention, not GPU/compositor present contention.
    last_timings: DecodeTimings,
}

/// Purely-CPU sub-stage timings for a single `decode_ref` call — see [`VideoPipeline::last_timings`].
///
/// `demux + send + receive` together are the old `h264` bucket, now split so we can tell whether
/// the stage that balloons under mouse-move load is file I/O (demux) or the frame-threaded decode
/// workers (receive) — the earlier round already established the whole `h264` bucket balloons ~100×
/// while `scale`/`copy` (main-thread userspace) stay flat.
#[derive(Clone, Copy, Default)]
pub struct DecodeTimings {
    /// Advancing `input.packets()` to the next packet: demux + the underlying file read. Blocking
    /// I/O; should be immune to CPU-scheduling pressure (page cache after the first playthrough).
    pub demux: std::time::Duration,
    /// `send_packet`: hands a compressed packet to the decoder. Usually near-instant (queues to
    /// the worker threads without blocking).
    pub send: std::time::Duration,
    /// `receive_frame`: pulls a decoded frame out. With frame-threading this *blocks on the worker
    /// threads*, so if the workers are being descheduled this is where the time goes.
    pub receive: std::time::Duration,
    /// `scaler.run`: swscale YUV→BGRA colour conversion of the one frame actually handed back.
    /// Single-threaded CPU work on the calling (main) thread, completely independent of the GPU.
    pub scale: std::time::Duration,
    /// `to_decoded_frame`: the ~W*H*4-byte allocation + per-row copy into a tightly-packed `Vec`.
    /// Also single-threaded CPU work on the calling thread.
    pub copy: std::time::Duration,
}

/// Default frame-decode worker count, capped to avoid oversubscribing hybrid CPUs during
/// interactive playback. Overridden by `FREEMUSIC_DECODE_THREADS`; see
/// `docs/implementation-notes.md`.
fn default_decode_threads() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(4)
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
        // for a scrub/playback pipeline that's never decoding "live".
        //
        // Keep the default capped instead of using libavcodec's "all logical CPUs" choice; the
        // history behind that cap is in `docs/implementation-notes.md`. `=0` restores libavcodec's
        // own pick for tuning/comparison.
        let thread_count = std::env::var("FREEMUSIC_DECODE_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or_else(default_decode_threads);
        context.set_threading(ffmpeg::threading::Config {
            kind: ffmpeg::threading::Type::Frame,
            count: thread_count,
        });
        let decoder = context.decoder().video()?;

        let width = decoder.width();
        let height = decoder.height();

        let mut scaler = ScalingContext::get(
            decoder.format(),
            width,
            height,
            Pixel::BGRA,
            width,
            height,
            Flags::BILINEAR,
        )?;
        apply_colorspace_details(
            &mut scaler,
            decoder.color_space(),
            decoder.color_range(),
            height,
        );

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
            last_timings: DecodeTimings::default(),
        })
    }

    pub fn duration_seconds(&self) -> f64 {
        self.duration_seconds
    }

    /// Purely-CPU sub-stage timings recorded during the most recent `decode_ref` call.
    pub fn last_timings(&self) -> DecodeTimings {
        self.last_timings
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
    /// seek. `exact` controls whether post-seek decode may stop at the first frame after the
    /// keyframe (`false`, for live scrubs) or must advance to `target_seconds` (`true`, for export
    /// and ordinary playback ticks). See `docs/implementation-notes.md`.
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
        self.last_timings = DecodeTimings::default();
        let need_seek = match self.current_frame.as_ref() {
            None => true,
            Some(frame) => {
                // An exact seek may legitimately land on the first frame at/after the requested
                // timestamp. Treat targets up to one frame before that cached frame as covered;
                // otherwise resuming from an exact seek can immediately classify the unchanged
                // transport as a backward jump and pay for another real seek.
                target_seconds + self.frame_duration_seconds + 1e-6 < frame.pts_seconds
                    || target_seconds - frame.pts_seconds > MAX_FORWARD_STEP_SECONDS
            }
        };

        if need_seek {
            // `Input::seek` calls `avformat_seek_file` with `stream_index = -1`, which means
            // the timestamp is in `AV_TIME_BASE` (microsecond) units, not this stream's own
            // time base.
            let target_ts = (target_seconds / f64::from(ffmpeg::rescale::TIME_BASE)) as i64;
            if let Err(err) = self.input.seek(target_ts, ..target_ts) {
                eprintln!("video seek failed: target_ts={target_ts} err={err:?}");
                return Err(err);
            }
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

        // Per-stage timers (see `DecodeTimings`) split this call's purely-CPU cost so the perf log
        // shows which stage balloons: `demux` (next packet / file read), `send` (`send_packet`),
        // `receive` (blocks on the frame-threaded decode workers), `scale` (swscale), `copy`
        // (readback). None touch the GPU, so this distinguishes I/O vs worker-thread vs main-thread
        // CPU contention. Timers accumulate across a whole catch-up burst, not just the shown frame.
        let mut demux_elapsed = std::time::Duration::ZERO;
        let mut send_elapsed = std::time::Duration::ZERO;
        let mut receive_elapsed = std::time::Duration::ZERO;
        let mut scale_elapsed = std::time::Duration::ZERO;
        let mut copy_elapsed = std::time::Duration::ZERO;
        let mut packets = self.input.packets();
        loop {
            let demux_start = std::time::Instant::now();
            let next_packet = packets.next();
            demux_elapsed += demux_start.elapsed();
            let Some((stream, packet)) = next_packet else {
                break;
            };
            if stream.index() != self.stream_index {
                continue;
            }
            let send_start = std::time::Instant::now();
            if let Err(err) = self.decoder.send_packet(&packet) {
                eprintln!("video send_packet failed: {err:?}");
                return Err(err);
            }
            send_elapsed += send_start.elapsed();
            let mut raw = VideoFrame::empty();
            loop {
                let receive_start = std::time::Instant::now();
                let received = self.decoder.receive_frame(&mut raw).is_ok();
                receive_elapsed += receive_start.elapsed();
                if !received {
                    break;
                }
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

                let scale_start = std::time::Instant::now();
                let mut scaled = VideoFrame::empty();
                if let Err(err) = self.scaler.run(&raw, &mut scaled) {
                    // AVERROR_INPUT_CHANGED: actual frame format/size differs from what the
                    // scaler was initialized with (common on Windows when coded dimensions or
                    // pixel format differ from what the stream parameters reported at open).
                    // Reinitialize from the frame's actual parameters and retry once.
                    let actual_fmt = raw.format();
                    let actual_w = raw.width();
                    let actual_h = raw.height();
                    self.scaler = ScalingContext::get(
                        actual_fmt,
                        actual_w,
                        actual_h,
                        Pixel::BGRA,
                        actual_w,
                        actual_h,
                        Flags::BILINEAR,
                    )
                    .map_err(|_| err)?;
                    apply_colorspace_details(
                        &mut self.scaler,
                        raw.color_space(),
                        raw.color_range(),
                        actual_h,
                    );
                    self.width = actual_w;
                    self.height = actual_h;
                    scaled = VideoFrame::empty();
                    self.scaler.run(&raw, &mut scaled)?;
                }
                scale_elapsed += scale_start.elapsed();

                let copy_start = std::time::Instant::now();
                let frame = to_decoded_frame(&scaled, pts_seconds);
                copy_elapsed += copy_start.elapsed();

                self.last_timings = DecodeTimings {
                    demux: demux_elapsed,
                    send: send_elapsed,
                    receive: receive_elapsed,
                    scale: scale_elapsed,
                    copy: copy_elapsed,
                };
                self.current_frame = Some(frame);
                return Ok(DecodedFrameRef {
                    frame: self.current_frame.as_ref().expect("frame was just cached"),
                    changed: true,
                });
            }
        }
        self.last_timings = DecodeTimings {
            demux: demux_elapsed,
            send: send_elapsed,
            receive: receive_elapsed,
            scale: scale_elapsed,
            copy: copy_elapsed,
        };

        if self.current_frame.is_none() {
            eprintln!(
                "video decode: packet loop exhausted with no frame (target={target_seconds:.3}s)"
            );
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
