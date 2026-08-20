# Architecture

Current system architecture for freemusic, split out of CLAUDE.md to keep that file short. See
also `docs/ui.md`, `docs/fmstyle-format.md`, `docs/verification.md`, and `docs/narratives/` for the
history and bug postmortems behind these decisions.

### Workspace layout

```
freemusic/
  Cargo.toml            # workspace root; pins wgpu ecosystem versions must stay in lockstep, see below
  app/                   # binary: winit + egui-wgpu shell
    src/main.rs           # event loop, AppState (owns everything), redraw/composite/present, export thread wiring
    src/gpu.rs             # wgpu Instance/Adapter/Device/Surface setup (interactive window only)
    src/ui.rs                # menu bar, tabbed side panel, timeline, calibration/crop/barrier drag handles
  crates/
    project/              # RON project model: paths, sync offset, calibration (incl. barrier), transform, styles
    video-pipeline/       # ffmpeg-next decode + seek, no GPU/UI dependency
    render/                # UI-agnostic compositor (video quad + note highway), used headless by export too
    mp4-encoder/            # forked ffmpeg-encoder: parameterized fps, explicit codec selection, optional audio
    export/                  # headless-GPU offline render loop, audio mux, progress/cancel channel
    audio-playback/          # cpal output stream for the loaded video's own audio, driven by transport position
  scripts/               # cargo check, run/screenshot/click/drag the app, gen synthetic test clips
```

### Neothesia-derived dependencies

`crates/render` depends directly on `midi-file` and `piano-layout`, both pinned git dependencies of
`PolyMeilex/Neothesia` at a fixed commit SHA (not `master`, per this project's "no semver safety
net" policy for external git deps). It has no dependency on `neothesia-core` — the note-highway
rendering pipeline is vendored in-tree (see `docs/fmstyle-format.md` and `crates/render/src/notes/`)
rather than reusing Neothesia's own `WaterfallRenderer`.

### Project crate: sync, calibration, persistence

`crates/project` is a serde/RON model: `Project { video_path, midi_path, sync_offset_seconds,
calibration, transform, style, background_color, ... }`, plus `KeyboardCalibration {
left_fraction, right_fraction }` (fractions of window width, 0.0-1.0, so a calibration survives a
window resize or reloading a differently-sized video) and `VideoTransform` (see below).
`Project::save`/`Project::load` return `Result<_, String>`.

Sync semantics: `midi_time = position_seconds - sync_offset_seconds`, computed once per redraw
before updating the note overlay. Video (plus its audio) is always the master clock — dragging the
sync offset only moves where notes render relative to it, never touches playback position.

Calibration is edited via drag handles directly on the video preview: two vertical lines
(`ui::draw_calibration_handles`) that mark the real keyboard's edges in the footage, implemented
with `ui.interact(rect, id, Sense::drag())` per handle and accumulated `Response::drag_delta()`.
The handles stop 60px above the bottom of the window so they don't compete with the transport bar
for drag input.

The sync offset / project controls live in a floating `egui::Window` ("Sync & Project"), not a
side panel — the video quad renders directly into the swapchain with no offscreen-texture
indirection at this layer, so a side panel would permanently cover a strip of it. `AppState` tracks
`applied_calibration` (the last `KeyboardCalibration` actually used to build note instances) and
only rebuilds the note layout when it differs from `ui_state.calibration`, avoiding a full rebuild
every redraw during an active drag.

Project save/load uses a typed path field plus Save/Load buttons, alongside the native Open/Save
dialogs described in `docs/ui.md`. `default_project_path` in `main.rs` prefills the field from the
loaded video's path (`song.mp4` -> `song.fmproj.ron`) the first time a video loads, without
overwriting a path the user already typed.

### Video transform: brightness/scale/crop/rotate/tilt/translate

`project::VideoTransform` holds `brightness`, `scale` (zoom), `rotation_degrees`,
`translate_x`/`translate_y` (pan), `tilt_x`/`tilt_y` (keystone), and a crop rect (`crop_left`/
`crop_right`/`crop_top`/`crop_bottom`, fractions of the source frame). The logic lives in
`crates/render/src/video_quad.rs`, applied in a single WGSL pass.

Everything except brightness and crop folds into one 3x3 homography matrix
(`video_quad::build_transform`), uploaded as a `mat3x3<f32>` uniform and applied to the quad's
local `(x, y, 1)` coordinates as `(x', y', w') = transform * (x, y, 1)`; the vertex shader feeds
`w'` into `clip_position.w` and lets the GPU's own perspective divide do the keystone distortion
(and perspective-correct the uv interpolation for free).

Matrix build order: `scale` (letterbox fit x user zoom) -> rotate (in an aspect-corrected space,
see below) -> translate (pan, in the same space the final rectangle sits in) -> tilt (last, so it
distorts the final on-screen rectangle rather than something that then gets rotated again).

Rotation needs an aspect-correction step: NDC's `x`/`y` axes don't correspond to equal physical
pixel counts unless the window is square, so `mat3_aspect` scales `y` by `1/window_aspect`
(`window_width/window_height`) before rotating (entering an isotropic space), rotates there, then
scales `y` back by `window_aspect` afterward to return to NDC.

The WGSL uniform buffer mirrors `mat3x3<f32>`'s column padding explicitly: in the uniform address
space each column is padded to 16 bytes (a `vec4`), so the Rust-side `Uniforms` struct uses
`[[f32; 4]; 3]` (`pad_columns` appends a trailing `0.0` per column), not `[[f32; 3]; 3]`.

Crop is UV remapping, not geometry: `crop_uv_min`/`crop_uv_max` remap the quad's `0..1` uvs to a
sub-rect of the texture. It also changes the effective aspect ratio fed into the letterbox `scale`
calculation in `update_viewport` (`video_w * crop_width_fraction` / `video_h *
crop_height_fraction`), so the letterbox matches the actually-visible (cropped) content.

The brightness uniform is read in the fragment shader (`color.rgb * uniforms.brightness`), so its
`BindGroupLayoutEntry` visibility is `ShaderStages::VERTEX | ShaderStages::FRAGMENT`.

Crop has no on-preview draggable overlay, unlike calibration — brightness/scale/rotation/tilt/
translate/crop are all Transform-tab sliders (`ui::draw_transform_tab`) only. The keyboard
calibration readout has its own matching sliders (`Keyboard left`/`Keyboard right`) alongside its
preview overlay (yellow, `ui::draw_calibration_handles`), since that geometry — a vertical strip
fraction of window width — doesn't depend on `VideoTransform` at all.

`update_viewport` runs unconditionally every redraw (one small uniform write is cheap), after the
egui pass, so a slider drag is reflected in the same frame's render rather than lagging by one.

### `wgpu`/`egui-wgpu` version pinning

`app/Cargo.toml` pins `wgpu = "29.0"` to match what `egui-wgpu 0.35` depends on internally — two
different `wgpu` major versions in the dependency graph cause hard type errors
(`Renderer::render` expecting a different `RenderPass` type). When bumping `egui`/`egui-wgpu`,
match whatever wgpu version they require rather than bumping `wgpu` independently.

### Data flow (video-pipeline)

All timing is `f64` seconds end-to-end, never frame counts, avoiding drift across mixed source
frame rates (23.976/29.97/30/60).

`VideoPipeline::seek_and_decode(target_seconds, exact)` is the clone-returning entry point used by
export/bench code that needs to own a frame. The interactive app uses
`seek_and_decode_ref(target_seconds, exact)` instead, which borrows the cached frame and returns
`DecodedFrameRef { frame, changed }` so playback can skip GPU texture uploads when the requested
timestamp is still covered by the previous source frame.

The decode path holds the last-decoded frame (`current_frame`) and skips touching the decoder
entirely if `target_seconds` is still covered by it — "covered" means `target_seconds < pts +
frame_duration_seconds`, treating a frame as valid for its full display interval rather than a
single instant. It only issues a real `Input::seek` for a backward jump or a forward jump bigger
than `MAX_FORWARD_STEP_SECONDS` (1.0s) — an actual scrub, not ordinary playback advancing a few
milliseconds per redraw. `exact=false` (scrub/preview) returns the first frame decoded after a
seek; `exact=true` (export, normal playback) decodes forward until the frame's timestamp reaches
the target.

`Input::seek` calls `avformat_seek_file` with `stream_index = -1`, so its timestamp argument is in
`AV_TIME_BASE` (microsecond) units (`ffmpeg::rescale::TIME_BASE`), not the stream's own
`time_base`.

`default_decode_threads()` caps the H.264 decoder's worker-thread count at
`min(available_parallelism, 4)`, overridable via `FREEMUSIC_DECODE_THREADS`.
`VideoPipeline::open` configures this via `context.set_threading(threading::Config { kind:
threading::Type::Frame, count })` before opening the codec (`Type::Frame`, not `Type::Slice`, since
consumer camera encoders typically write one slice per frame).

`seek_and_decode`'s catch-up loop (used when `exact = true`) only runs the per-frame YUV->BGRA
swscale conversion and buffer copy for the frame that's actually about to be returned — discarded
intermediate frames (needed for their raw H.264 decode regardless, since P/B-frames require their
references decoded either way) skip straight to the next `receive_frame` via `if exact &&
pts_seconds < target_seconds { continue; }`.

`apply_colorspace_details` (`crates/video-pipeline/src/lib.rs`) is called after each
`ScalingContext::get` (in `open` and in the `AVERROR_INPUT_CHANGED` reinit path in `decode_ref`):
it maps the decoded frame's/decoder's `color::Space` to the matching `SWS_CS_*` constant (falling
back to a resolution-based guess — BT.709 for `height >= 720`, else BT.601 — when the stream
doesn't tag a colorspace), reads `color::Range` to determine limited vs. full range, and calls
`ffmpeg_next::ffi::sws_setColorspaceDetails` directly (via the crate's `as_mut_ptr()` escape
hatch, since `ffmpeg-next` doesn't wrap it safely). `dstRange` is always passed as full (1) since
the destination is BGRA, which has no limited-range encoding of its own.

`piano_layout::KeyboardLayout::from_range` lays keys out starting at local x=0 and has no offset
parameter and no support for a truncated range that starts mid-octave. Keyboard-calibration
alignment (`midi_overlay`/note-highway code) instead sizes the layout to the calibrated width and
shifts every built note instance's x position right by the calibrated left edge in pixels, applied
after both initial construction and any resize. Segment layout (used for per-octave calibration —
see `docs/ui.md`) queries a segment as a suffix of the standard 88-key layout (segment start
through the real keyboard end, then discards trailing keys with `take(segment_len)`), avoiding
`from_range`'s mid-octave truncation limitation.

### Rendering (app)

`Gpu` (`app/src/gpu.rs`) owns the wgpu `Instance`/`Device`/`Queue`/`Surface` for the interactive
window; instance creation goes through
`wgpu::InstanceDescriptor::new_without_display_handle_from_env()` so `WGPU_BACKEND` and similar env
vars are respected. `export::gpu::HeadlessGpu` is the analogous struct for the export render loop,
minus the `Surface`/`config`.

`render::video_quad::VideoQuad` (`crates/render/src/video_quad.rs`) is a self-contained
aspect-correct textured-quad pass: uploads the latest `DecodedFrame`'s BGRA bytes to a
`wgpu::Texture` (recreated only when the frame size changes) and computes a letterbox/pillarbox
scale uniform from `(video_size, window_size)` each frame. It renders via 6 hardcoded vertices in
the shader (no vertex buffer). `render::Compositor` wraps it together with the note-highway
renderer, and `Compositor::render` draws in a fixed order each frame: video quad, then octave
lines, then notes, then barrier, then transition effects (particles/flashes).

The interactive preview renders into an offscreen `Rgba8Unorm` texture
(`app/src/main.rs::PREVIEW_TEXTURE_FORMAT`), a requirement of
`egui_wgpu::Renderer::register_native_texture`. Because that target isn't sRGB, it doesn't
auto-encode linear->gamma on store, so `VideoQuad` carries a `manual_srgb_encode` uniform flag
(`!surface_format.is_srgb()`) and `shader.wgsl` calls its own `linear_to_srgb` in `fs_main` when
the flag is set. Export renders to `Bgra8Unorm` (non-sRGB, matching the preview's format), so the
same manual-encode path applies there too, and every draw (video quad, notes, barrier, particles)
blends identically in both places by construction.

The note-highway/barrier renderers (`crates/render/src/notes`, `barrier.rs`) each use two GPU
pipelines per layer: a glow/corona pass drawn first, then an opaque/alpha-blended core pass drawn
second so the core occludes the glow beneath its own footprint. Transition particles/flashes
(`effects.rs`) use a single pipeline, since they have no separate opaque core to occlude — additive
particles/flashes use the same corona math as the glow pass, and non-additive puff particles use a
premultiplied-alpha path with a hard edge instead. All three glow-producing pipelines (barrier,
notes, effects) use GPU screen blending (`src=OneMinusDst, dst=One, Add`) rather than
linear-additive blending, so overlapping glows/flashes desaturate toward white instead of clipping
to a flat white plateau on the 8-bit UNORM render target. That desaturation only works once each
draw's own `src` is already near the `[0, 1]` range, though — `effects.wgsl`'s `core_strength` is
intentionally unbounded toward the center of a flash/particle's `core_radius` (see its doc comment),
routinely reaching several times `1.0` well before the boundary, so `fs_glow` maps its resolved
`color * strength_rgb * alpha` through `1.0 - exp(-x)` before returning it — a per-draw tonemap
compressing toward `1.0` instead of relying on the 8-bit UNORM target's hardware clamp, which would
otherwise flatten that whole bright interior into a hard-edged solid disc (see
`docs/narratives/architecture.md`). Unlike the note-highway/barrier pipelines (still plain
per-instance vertex-buffer attributes), `effects.rs`'s per-instance data (`EffectInstance`) is read
from a storage buffer (`effects.wgsl`'s `instances`, bound at group 1, indexed by
`@builtin(instance_index)` in `vs_main`) rather than vertex attributes — the only vertex buffer left
on this pipeline is the shared unit quad's own `position` at location 0. This sidesteps wgpu's
16-vertex-attribute-location cap entirely (the vertex-attribute version had already grown up against
it — see `docs/narratives/architecture.md`), at the cost of every `EffectInstance`/`effects.wgsl`
`Instance` field needing to stay a plain `vec4<f32>` (WGSL's storage-buffer struct layout gives
`vec4<f32>` matching 16-byte alignment and size, so consecutive fields pack with no implicit padding
gap the tightly-packed `#[repr(C)]` Rust side wouldn't otherwise have — see `EffectInstance`'s own
doc comment). Transition effects (particle pools) are
stateful across redraws — position depends on
elapsed time, velocity, gravity, spawn time, and RNG state — so the update loop tracks the previous
transport time, advances the pool by the delta, spawns bursts for note arrivals crossed since the
last update, and clears the pool on large timeline jumps (there's no single correct mid-scrub
transient state to reconstruct).

`AppState::redraw` in `main.rs` is the per-frame orchestrator: advance/clamp the transport position
and decode -> update MIDI -> run egui (`ui::draw`) -> if egui queued a seek, consume and decode it
immediately in that same redraw -> apply calibration/project/export changes -> sync audio ->
render/present. `compositor.upload_frame` only runs when `DecodedFrameRef::changed`. Export
progress chains `window.request_redraw()` directly at the end of `redraw`; playback instead sets
`next_playback_redraw_at`, and `ApplicationHandler::about_to_wait` uses `ControlFlow::WaitUntil` to
wake at the loaded video's own `frame_duration_seconds` — scheduled from `frame_start` when the
frame finishes before its cadence deadline, or from `Instant::now()` plus one frame interval if
render already overran the deadline (so the app doesn't try to catch up by immediately drawing
several stale frames). `redraw` also clamps each frame's position-advance step to
`MAX_PLAYBACK_DT_SECONDS`, bounding how far a single slow redraw can push the position forward.

Rendering uses two passes per frame: a `scene_pass` (`LoadOp::Clear`) draws `compositor` (video
quad then note highway) with a normally-scoped (non-`'static`) `RenderPass`, followed by an
`egui_pass` (`LoadOp::Load`, compositing on top without clearing) using
`RenderPass::forget_lifetime()` for egui-wgpu's `'static` requirement.

`window_event`'s top-of-function repaint nudge (`if response.repaint { window.request_redraw();
}`, driven by `egui_state.on_window_event`) excludes `WindowEvent::RedrawRequested` itself and
excludes passive `CursorMoved` while `ui_state.playing` and no mouse button is held (tracked via a
`pointer_buttons_down` counter, reset on focus loss) — playback's own redraw scheduling
(`about_to_wait`/`next_playback_redraw_at`) governs the frame cadence instead, and real
drags/scrubs still request immediate repaints.

Audio play/pause sync (`AudioPlayback::set_playing`) runs after the egui pass and queued UI actions
in `redraw`, so a transport button click affects the audio stream in the same redraw that processes
the click. A one-shot extra redraw (`playing_before_ui`/`playing_changed_by_ui`) repaints
immediately after a play/pause click so the button label updates without waiting for further input.

### MP4 export

`crates/render` (video quad + note-highway compositor) is shared between the interactive app and
`crates/export`'s headless render loop, via `render::GpuHandles<'_>` (borrowed `instance`/
`adapter`/`device`/`queue`/`texture_format`) so the same `Compositor` works against either
`app::gpu::Gpu` (has a `Surface`) or `export::gpu::HeadlessGpu` (doesn't).

`crates/mp4-encoder` is a fork of Neothesia's `ffmpeg-encoder`, with these deltas from upstream:
- `fps: u32` is a parameter to `mp4_encoder::new(..)` (not a hardcoded `60`), and `gop_size` scales
  with it (one keyframe/sec).
- Codec selection is explicit: `ff::Codec::find_by_name(c"libx264")` / `c"aac"`, falling back to
  `output_format.video_codec()`/`audio_codec()` if absent.
- Audio is optional (`with_audio: bool` on `mp4_encoder::new`) — a video with no audio stream skips
  creating an audio codec/stream entirely rather than encoding silence.
- `EncoderInfo` has `sample_rate: i32` (the codec's actual chosen rate, read back after
  construction); `crates/export`'s audio resampler targets this rather than assuming a fixed rate.
- `ffmpeg-sys-next` is a plain crates.io dependency (not a git dependency), resolving to the same
  `8.1.0` version `ffmpeg-next` (video-pipeline's decoder) pulls in.

`crates/export::run(project, settings, progress, cancel)` blocks for the whole export and is meant
to be driven from a background thread; it takes `Project` by value so the spawned closure can
`move` an owned snapshot. Canvas size is the source video's own decoded width/height (rounded down
to even, since yuv420p requires it), not the interactive window size. The GPU texture readback
buffer's row stride is padded to `COPY_BYTES_PER_ROW_ALIGNMENT` (256) as `wgpu` requires, then
stripped back to tightly-packed BGRA per row before handing frames to the encoder.

Audio is decoded via a second, independent `ffmpeg_next::format::input` open
(`crates/export/src/audio.rs`), separate from `video-pipeline`'s `VideoPipeline`.
`audio::has_audio_stream` is a cheap pre-check used to decide `with_audio` before the encoder (and
therefore its `sample_rate`) exists; the real decode+resample (`audio::decode_all`, using
`ffmpeg_next::software::resampling::Context`) happens once upfront, targeting that `sample_rate`,
and the export loop drains `EncoderInfo.frame_size`-sized chunks into `Frame::Audio` calls per
output frame. A video with no audio stream constructs the encoder with `with_audio: false`.

UI: a floating "Export" window (`ui::draw_export_window`) holds the output path text field
(defaulted to `<video_stem>_export.mp4`), an fps `DragValue`, and swaps an "Export" button for a
progress bar + "Cancel" button once export is running. `AppState::start_export` snapshots a
`Project` and spawns the background thread; `redraw` drains progress messages each frame with a
non-blocking `try_recv` loop and clears the run once a `Done`/`Cancelled`/`Error` message arrives.
The end-of-`redraw` `window.request_redraw()` condition includes `|| self.export_run.is_some()` so
the progress bar keeps advancing even while playback is paused.
