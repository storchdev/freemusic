# Architecture: what worked and gotchas found

Narrative companion to `docs/architecture.md` — the bug postmortems and design decisions behind
the current architecture, roughly in the order they happened.

## Neothesia reuse, before the note pipeline was vendored (superseded by Phase B of the
`.fmstyle.ron` milestone)

Before the note-highway pipeline was vendored in-tree (see `docs/narratives/fmstyle-milestone.md`
Phase B), `crates/render` depended on `neothesia-core` and wrapped its `WaterfallRenderer`
directly, in a now-deleted `crates/render/src/midi_overlay.rs`. This section records why several
things ended up shaped the way they still are today.

`crates/render/Cargo.toml` depended on `midi-file` and `neothesia-core` as git deps pinned to an
exact commit SHA of `PolyMeilex/Neothesia` (`e61639b12cc8e466b90406c564da5f9f54d8d1a3`, fetched
2026-06-30) — never `master`, per the project plan's "no semver safety net" risk.
`neothesia-core` re-exports `wgpu_jumpstart::{Gpu, TransformUniform, Uniform, Color}` and the whole
`piano_layout` crate at its root, so those didn't need separate git-dep entries at the time. Both
resolved to `wgpu 29.0.4`, matching this project's own pin — verified by `cargo build` producing a
single `wgpu` entry in `Cargo.lock`.

`midi_overlay.rs` wrapped `neothesia_core::render::WaterfallRenderer`. `WaterfallRenderer::new`/
`resize` took a `&neothesia_core::Gpu` (`wgpu_jumpstart::Gpu`), a different type from either of
this project's own GPU structs (the interactive window's `app::gpu::Gpu`, or export's headless
`export::gpu::HeadlessGpu`). Rather than tying `midi_overlay` to one of those, `midi_overlay::
wrap_gpu` built a `neothesia_core::Gpu` on the fly from a `render::GpuHandles<'_>` — the small
struct of borrowed `wgpu::Instance`/`Adapter`/`Device`/`Queue` refs plus a `TextureFormat` that
survives in today's architecture (`docs/architecture.md`'s MP4 export section) precisely because
it was designed to work regardless of which side built the underlying GPU struct.
`neothesia_core::Gpu`'s fields were all `pub` with no constructor invariants, so cloning the
cheap `Arc`-backed handles into it worked regardless of origin.

`neothesia_core::config::Config::default()` was used as-is rather than `Config::new()`, which would
have read `~/.config/neothesia/settings.ron` if the user happened to have real Neothesia installed
— harmless (read-only, falls back to defaults) but an unnecessary external coupling that wasn't
needed since this project never called `.save()`.

**Keyboard calibration had no support in `piano_layout` itself** — `KeyboardLayout::from_range`
always laid keys out starting at local x=0, with no offset parameter, and still does. Rather than
forking `piano-layout`, `midi_overlay::keyboard_layout` sized the layout to fit the *calibrated
width* (`(right_fraction - left_fraction) * window_width`, not the full window width), and a
separate helper `apply_left_offset` shifted every already-built `NoteInstance.position[0]` right by
the calibrated left edge in pixels, then re-uploaded via `WaterfallPipeline::prepare` — both
`WaterfallRenderer::pipeline()` and `WaterfallPipeline::instances()`/`prepare()` were plain public
methods, so this needed no upstream changes. This same width-then-shift approach is what today's
vendored note pipeline still does (`docs/architecture.md`'s Data flow section).

## Video transform: the rotation-swap bug

The two `mat3_aspect` scale factors (pre-rotate `1/window_aspect`, post-rotate `window_aspect`)
were originally implemented swapped — backwards from the correct derivation. It was reported as
"the rotate slider warps the footage severely" rather than rotating it normally. Confirmed
algebraically, not just by eyeballing: tracing a purely-horizontal NDC point through a 90° rotation
with the swapped factors scaled its resulting vertical pixel offset by `window_aspect²` relative to
the correct answer instead of preserving magnitude — e.g. at `window_aspect = 2` (a 2:1 wide
window) an 800px horizontal offset became 200px vertical instead of 800px, a 4x error, growing or
shrinking with how far the window departs from square. `cargo build`/`clippy` can't catch a
wrong-but-type-correct matrix composition like this — it needs a manual re-check (drag the rotation
slider on a non-square window/video and confirm the footage rotates rigidly, edges staying
straight, rather than skewing) whenever this code is touched.

## The bind-group-layout-visibility panic

Brightness is applied in the fragment shader, but the uniform buffer's `BindGroupLayoutEntry` was
initially left at `visibility: ShaderStages::VERTEX` only, unchanged from when the uniform held
only the vertex-only letterbox scale. This built fine but panicked at pipeline-creation time on
first run (`wgpu error: ... Shader global ResourceBinding ... is not available in the pipeline
layout ... Visibility flags don't include the shader stage`) — `cargo build`/`clippy` can't catch
this kind of mismatch, it only surfaces from actually running the app.

## The cyan on-preview crop-box overlay: built, then removed

An on-preview crop-box overlay (`ui::draw_crop_handles`) once existed alongside the calibration
handles. It never actually tracked the real, on-screen video: it positioned the box using only the
four crop fractions against the raw `image_rect`, as if the video always exactly filled it with
`scale == 1.0` and no translate. The real video quad's position also depends on `scale`,
`translate_x`/`translate_y`, and crop's own effect on the letterbox aspect, so the box quietly
stopped matching the video the moment any of those moved off their defaults — reported as "the top
of the cyan box is much higher than the top of the video" (a `scale < 1.0` project centers a shrunk
video with visible margin on every side; the box, oblivious to `scale`, still hugged the
untransformed frame's edges).

A first attempt added a `video_display_rect` helper replicating `video_quad::update_viewport`'s
exact letterbox-from-crop-aspect formula plus `scale`/`translate_x`/`translate_y`, and repositioned
the box/handles against that — correct for scale/translate, but still deliberately not accounting
for `rotation_degrees`/`tilt_x`/`tilt_y` (a rotated/tilted quad isn't axis-aligned, so it can't be
represented by a plain `egui::Rect` without a polygon overlay and a rework of the edge-drag
hit-testing). Rather than keep that remaining rotation/tilt gap around, the whole overlay
(`draw_crop_handles`, `video_display_rect`, and their call site) was deleted from `app/src/ui.rs` —
crop is edited via the Transform tab's sliders only, same as the other transform fields, which is
today's current behavior. `CROP_MIN_GAP` (the `crop_right - crop_left >= 0.1` guard, shared with
those sliders) was unaffected by the removal.

## RESOLVED: "playback goes very laggy whenever the mouse moves anywhere over the window"

Root cause: a **hybrid-core scheduling artifact** on the dev machine (Intel Core Ultra 7 258V,
Lunar Lake — 8 cores, no SMT: CPUs 0–3 are fast P-cores w/ shared L3, CPUs 4–7 are slow LP-E cores
w/ no L3, per `lscpu -e`). The H.264 decoder was opened with `threading::Config { count: 0 }`, which
lets libavcodec spawn ~one frame-decode worker per logical CPU (8+). Under the ~1000Hz
`CursorMoved` event/system churn from moving the mouse, the scheduler kept the app's main thread on
a P-core but descheduled the decode *workers* onto (or off) the slow LP-E island. That surfaced as
`avcodec_send_packet` blocking while it waited for a not-yet-free worker slot — measured ~150x
(~200us -> 20-30ms/frame), i.e. multi-second playback lag for as long as the mouse moved.

**Primary fix**: cap the decode worker count (`default_decode_threads()` = `min(available_parallelism,
4)`, overridable by `FREEMUSIC_DECODE_THREADS`) — now the current, permanent behavior described in
`docs/architecture.md`. Verified by an on-machine sweep with the mouse moving: `count: 0` -> `send`
20-30ms; `=1` -> ~5-8ms (works, thin headroom, too slow for real footage); `=2` -> ~3-5ms; `=4` ->
~150us, flat as steady-state — lag gone, because 4 workers stay resident on the 4 P-cores and never
spill to the LP-E cores or oversubscribe, with no loss of steady-state throughput. `=0` restores
the old pick-everything behavior (reproduces the bug on such a CPU).

The cap is safe on other machines (fewer-core boxes get <=4 anyway; `available_parallelism`
respects cgroup/affinity limits) — the only mild downside is that `export` (offline, decode-heavy,
no mouse contention) also opens its own `VideoPipeline` and so is capped at 4 too, leaving some
cores idle on a big all-P-core machine. Not worth fixing; if it ever matters, the export path could
request a higher/uncapped thread count so only interactive playback stays capped.

**Secondary fix**: during playback the video cadence should govern; `next_ui_redraw_at` must not
schedule a redraw sooner than the next frame. Passive mouse movement makes egui request a hover
repaint every update, which flows through `next_ui_redraw_at` and bypasses the
`passive_playback_cursor_move` guard in `window_event` (that guard only suppresses the *direct*
`request_redraw` nudge, not the egui-animation deadline path). Left unclamped it drove the redraw
rate to 40-54fps on a 30fps clip — wasted full redraws that decode no new frame. This alone did
*not* fix the lag (a thread-capped run is smooth even at the inflated fps), but it removes real
waste, so the clamp stayed. While paused, `next_ui_redraw_at` governs fully (smooth menu/panel
animations); while playing, egui animations advance at the frame cadence (imperceptible).

**Also kept**: the `MAX_PLAYBACK_DT_SECONDS` one-frame `dt` cap in `redraw` (from an earlier
session, described as current behavior in `docs/architecture.md`) — bounds the runaway spiral where
a slow redraw inflates the next redraw's catch-up. It was never the fix for this particular bug but
is a correct cheap guard worth keeping regardless.

How it was diagnosed (a method worth reusing for future playback-perf bugs): the pre-existing
`[perf]` log (`PerfStats::maybe_print`) already showed `decode` ballooning while the GPU-touching
timers `acquire`/`render_submit` stayed low — killing an earlier "GPU/compositor contention"
theory. A temporary per-stage split of `decode` (timers around demux / `send_packet` /
`receive_frame` / swscale / readback-copy) then showed the main-thread userspace stages (swscale,
copy) stayed flat while only `send_packet` blew up — the asymmetry that pinpointed worker-thread
starvation over global CPU load, and pointed straight at the thread-count fix. That split was
diagnostic scaffolding, trimmed back to the aggregate `decode` timer afterward. Ruled out along the
way: Xwayland (reproduces under native Wayland too) and Hyprland's cursor path
(`cursor:no_hardware_cursors`/`use_cpu_buffer` toggles had no effect).

## Video decode timing: the reseek and catch-up bugs

`VideoPipeline::seek_and_decode(..., exact)`'s split between explicit scrubs and ordinary playback
ticks (now documented as current behavior in `docs/architecture.md`) exists because reseeking every
redraw would land on/near the *same* nearest keyframe every time for any video with a keyframe
interval longer than a redraw's time delta, freezing playback at the keyframe instead of animating.
This was a real bug, caught by screenshotting a burned-in frame counter mid-playback — static color
test patterns wouldn't have revealed it, which is why `scripts/gen-test-video.sh` still generates a
clip with a visible per-frame counter and a multi-second keyframe interval today.

A second bug in the same area: catch-up bursts (`exact = true` decoding forward to reach
`target_seconds`) scaled+copied every discarded intermediate frame, not just the one actually
shown. Reported as "playback of a real ~1080p30 camera clip is laggy, one CPU core pegged" even
after the decode-threading fix above (later, once threading also brought all 8 cores into it: "all
cores spike, still laggy"). Root-caused with a temporary call counter in `seek_and_decode` (added,
used, then stripped back out once the fix was confirmed): logging showed `decodes/s` in the
hundreds while `calls/s` (redraws that actually touched the decoder) was only 6-40, i.e. dozens of
frames decoded per call — the catch-up loop working as designed, but every loop iteration,
including every discarded intermediate frame, was still paying for a full `self.scaler.run`
(YUV->BGRA swscale over the whole frame) *and* a fresh ~8MB `Vec` allocation+copy in
`to_decoded_frame`, immediately thrown away the instant the next `receive_frame` succeeded. A
second counter (gap between `target_seconds` and the held frame's `pts_seconds`, plus wall-clock
time per call) showed *why* this compounded instead of staying a one-off hiccup: gap and call
duration grew in lockstep call over call (e.g. 0.079s/80ms -> 0.199s/220ms -> ... -> 0.466s/485ms
before a hard reseek reset it) — a real feedback spiral, since `position_seconds` advances by
wall-clock `dt` every redraw regardless of how long the *previous* redraw's decode took, so a slow
catch-up call directly inflated the gap the *next* call had to close.

Fixed by moving the scale+copy after the "have we reached target yet" check instead of before it —
raw H.264 decode still happens for every frame in the burst (needed regardless, since P/B-frames
require their references decoded either way), but the expensive per-frame conversion only happens
once, for whichever frame actually reaches the caller — the current behavior described in
`docs/architecture.md`. Measured end-to-end on a 1080p30 clip: process CPU during playback dropped
from 300%+ (main thread ~80%, eight `av:h264` worker threads ~25-35% each) to ~85%. Some residual
periodic ~100ms stutter remained even after this fix in testing — not root-caused at the time;
worth checking GPU-present/window-occlusion behavior (see the unthrottled-redraw section below)
before assuming it's decode-related, since this machine has a documented history of
occlusion-driven multi-hundred-ms `Surface::get_current_texture` stalls that look identical from
the decode side.

## Video played back visibly darker than reference players

Root cause: `ScalingContext::get` (`sws_getContext`) only takes pixel format and dimensions — no
colorspace/color-range parameters — so swscale silently used its own hardcoded default (BT.601
matrix, limited/MPEG 16-235 range in and out) regardless of what the source actually was.
Camera-originated footage (phones especially) is very commonly BT.709 and/or full-range, so without
an explicit `sws_setColorspaceDetails` call, values stayed compressed toward the middle of the
8-bit range compared to players that read and correct for the stream's real color metadata. Fixed
by `apply_colorspace_details` (now the current, permanent behavior described in
`docs/architecture.md`), called right after each `ScalingContext::get`.

## Interactive preview darker than mpv/iOS, on top of (not fixed by) the above

The colorspace/range fix above made `video-pipeline`'s BGRA output match `ffmpeg`'s own reference
conversion exactly (verified with a throwaway `dump_frame` example comparing mean RGB against
`ffmpeg -noautorotate -ss <t> -frames:v 1 -pix_fmt rgb24` on the same timestamp — they matched to 2
decimal places), but the app still looked dark, meaning the real bug was downstream in rendering,
not decode. Root cause: the video texture (`Bgra8UnormSrgb`) is correctly sampled — `textureSample`
auto-decodes sRGB->linear — but since the 6c UI restructure the compositor renders into the
offscreen preview texture, forced to `Rgba8Unorm` because `egui_wgpu::Renderer::
register_native_texture` requires exactly that format, not an sRGB format. A non-sRGB render
target does *not* auto-encode linear->gamma on store, so the fragment shader was writing
linear-space color directly into it — those bytes then got read back (by egui, and ultimately the
display) as if they were already gamma-encoded, crushing every midtone dark (e.g. 50% linear gray
stores as ~128/255 where correct sRGB-encoded 50% gray is ~188/255). Export was unaffected (it
rendered to `Bgra8UnormSrgb` at the time, which does auto-encode correctly), which is why this was
preview-only. Fixed by adding the `manual_srgb_encode` uniform flag and `linear_to_srgb` shader
function that are now the current, permanent implementation.

## Exported notes/barrier/particles looked blown-out/washed-white compared to the preview

Reported directly by comparing an editor screenshot against the corresponding exported frame —
notes lost their distinct per-key color and rounded shape, the green barrier line and falling
particles disappeared entirely into the glow. Root cause: unlike `video_quad`, the notes/barrier/
effects pipelines had no `manual_srgb_encode` compensation at all — they converted their hex colors
sRGB->linear once on upload and then blended many overlapping additive draws (note fill/glow,
particles, barrier glow) directly against whatever format the render target was, relying on the
target itself being non-sRGB so the linear-ish blended sums land in the output bytes unmodified —
matching how the interactive preview's `Rgba8Unorm` offscreen texture already behaved. Export's
offscreen texture was `Bgra8UnormSrgb` though (chosen when export was first added, before the 6c
preview even existed to compare against) — a real sRGB target auto round-trips every blended draw
through a decode/blend/encode cycle, and because many notes/glow/particle layers stack per frame,
that round-trip compounded each time, pushing highlights toward clipping and crushing the thinner
barrier/particle layers into the blown-out background.

Fixed by changing export's offscreen format to `Bgra8Unorm` (the non-sRGB sibling, matching the
interactive preview's format exactly) instead of adding per-shader compensation to three more
pipelines — this also flipped `video_quad`'s existing `manual_srgb_encode` flag on for export
(since `!format.is_srgb()` became true there too), so every layer blends identically in both places
by construction rather than by parallel-maintained flags. This is the still-current design
described in `docs/architecture.md`.

## Two render passes instead of one (changed at milestone 2)

Milestone 1 used a single render pass for video quad + egui, but
`WaterfallRenderer::render<'rpass>(&'rpass mut self, pass: &mut RenderPass<'rpass>)` tied its
`&mut self` borrow to the pass's lifetime *parameter*, and `wgpu::RenderPass` is invariant over
that parameter — so it couldn't share a pass that had already been `forget_lifetime()`'d to
`'static` (the borrow checker error was "borrowed data escapes outside of method... argument
requires that `'1` must outlive `'static`"). Splitting into a normally-scoped `scene_pass` and a
`forget_lifetime()`'d `egui_pass` — still today's structure — kept the scene pass's lifetime real,
which `WaterfallRenderer` was fine with. The project plan's longer-term offscreen-texture design
(decoupling preview resolution from window size, since implemented in the 6c milestone) would have
sidestepped this differently, but wasn't required to unblock milestone 2's compositing at the time.

## Fixed: unthrottled redraw loop pegging the GPU/CPU at all times

**Symptom**: the app was "incredibly laggy" — reported generally, not tied to any specific
interaction. Root cause turned out to have nothing to do with decode speed, texture upload size, or
egui overhead (all measured and found unremarkable); it was the *idle* state that was broken.
Diagnosed by adding temporary `Instant`-based timing around every stage of `redraw` plus a periodic
tally of `WindowEvent` variants received, run against `scripts/gen-test-video.sh`'s synthetic clip.
The event tally was what actually revealed it: `WindowEvent::RedrawRequested` was firing ~120
times/sec (matching the machine's display refresh rate) *before any video was even loaded or
played*, with no other input events driving it — the window was continuously repainting forever at
full vsync rate regardless of `ui_state.playing`, burning a full
decode-check/egui-run/tessellate/compositor-render/egui-render/submit/present cycle every ~8ms,
24/7, for no reason. (The `acquire` — `Surface::get_current_texture` — stage dominated each of
these idle frames at ~7-8ms, which is expected Vulkan FIFO/vsync blocking for a
continuously-presenting loop, not itself a bug; the bug was that the loop was continuous at all
when nothing had changed.)

**Cause**: `window_event`'s top-of-function generic repaint nudge ran for *every* incoming
`WindowEvent`, including `WindowEvent::RedrawRequested` itself. `egui-winit`'s `on_window_event`
returns `repaint: true` for `RedrawRequested` along with most other events — its own doc comment
frames this as "a repaint just happened, the platform may want another one queued", deliberately
leaving the actual repaint *policy* up to the caller. This app's policy already existed and was
correct — `redraw`'s own last lines only called `window.request_redraw()` when `ui_state.playing ||
export_run.is_some()` — but the generic top-of-`window_event` check bypassed that policy entirely:
handling a `RedrawRequested` event synchronously queued the *next* `RedrawRequested`, forever,
independent of `redraw`'s own end-of-frame decision. A self-sustaining loop with no way to stop
itself once started (which was immediately, at the first paint after window creation).

**Fix**: exclude `WindowEvent::RedrawRequested` from the generic nudge, and also exclude passive
`CursorMoved` while `ui_state.playing` and no mouse button is held — both now permanent, current
behavior described in `docs/architecture.md`. The first part prevents a self-sustaining redraw
loop; the second prevents a user simply moving the mouse over the window during playback from
turning playback back into an input-rate redraw loop that bypasses the video-frame scheduler and
causes stutter. Verified with the same event-tally instrumentation: idle `RedrawRequested` rate
dropped from ~120/sec to ~0-1/sec (only real input causes a redraw now), and idle CPU for the
process dropped from continuous background load to ~1-2% (`ps -o pcpu`). Playback still
self-sustains its own redraw loop correctly, confirmed by playing a 30s synthetic clip end-to-end
and watching the on-screen frame counter and transport position advance smoothly and land exactly
on the expected frame for elapsed wall-clock time.

**Play/Pause ordering gotcha found in the same session**: audio play/pause synchronization needs to
happen *after* the egui pass and queued UI actions in `AppState::redraw`, not before — this is now
the current, permanent ordering. The transport button toggles `ui_state.playing` inside `ui::draw`;
syncing `AudioPlayback::set_playing` before that pass left CPAL in the previous state until a later
repaint. The visible failure mode was clicking Pause while the cursor still hovered the button:
hover/input repaints could make the frame/audio feel like it was jackhammering until the cursor
left. Post-UI audio sync makes the button click affect the audio stream in the same redraw that
processes the click. The `playing_before_ui`/`playing_changed_by_ui` one-shot redraw exists for the
same reason: the button label is computed before the click is processed, so the click frame still
paints the old label and needs one immediate follow-up frame to show "Play"/"Pause" without waiting
for the cursor to move.

**A separate, unrelated observation from the same debugging session, not itself a bug**: while
testing playback under this environment's background-job Xwayland session, `acquire` was once seen
blocking for a full ~1 second per frame, persisting across many consecutive frames. This tracked
exactly with the app window being tiled/occluded by Hyprland (not floated) rather than any
app-level issue: floating, resizing, and centering the window via `hyprctl` before retesting made
it disappear completely, consistent with compositors throttling swapchain presentation for
occluded/non-visible surfaces. Worth remembering if a future perf report mentions "playback stalls
for about a second at a time" specifically — check window occlusion/focus state before assuming a
decode or render regression.

## Barrier and transition glow: architectural history

The barrier renderer began as an egui overlay and moved into a wgpu pass so the same compositor
could render both the interactive preview and exported video. The "white-hot pipe" redesign removed
separate glow and pulse intensity knobs in favor of brightness driving the core color too,
desaturating toward white above `1.0` — this kept a bright barrier from reading as a flat bar with
a disconnected colored ring around it. The additive corona redesign after that replaced a single
alpha-blended halo with a sum of three exponential falloff layers, and introduced the additive/
opaque-core two-pass split that's now permanent (`docs/architecture.md`'s Rendering section);
`show_bar` independently controls the core so a barrier can be pure glow, pure bar, both, or
neither. See `docs/narratives/fmstyle-history.md` for the numeric detail of this redesign's three
generations.

## Flash core rendered as a solid disc instead of a bright point, and appeared not to decay

Reported after the aurora-corona port (see `docs/narratives/fmstyle-history.md`'s breaking-change
log): with the shipped `aurora-corona.fmstyle.ron` sample's settings (`radius_x_px`/`radius_y_px:
14`, `layers` amplitudes `2.05`/`1.75`/`0.6`), the flash's core rendered as a flat, hard-edged solid
disc filling the whole `core_radius` ellipse, and visibly stopped responding to `decay_seconds` for
most of the flash's life, only fading right at the very end.

`effects.wgsl`'s `core_strength` is deliberately unclamped in its interior (see that function's own
doc comment — an earlier fix removed a literal flat-plateau clamp so brightness would rise
continuously toward the center instead of stopping at a flat disc). But "continuously" doesn't mean
"boundedly": `edge_dist_px` reaches `-radius` at the center, so `exp(-edge_dist_px / sigma)` grows
to `exp(radius / sigma)` there — with this sample's radius/sigma ratio that's routinely 4-30x per
layer, amplitudes included. `fs_glow` writes straight to `Rgba8Unorm` (the interactive preview's
format, and `Bgra8Unorm` for export — see the blown-out-export section above) via plain additive
blending, with no HDR intermediate target or tonemap pass anywhere in the pipeline. The GPU
therefore hard-clamps every channel over `1.0` on write — which reproduces the *exact* flat, hard-
edged disc the earlier plateau fix was written to eliminate, just via the output format's clamp
instead of the formula's. It also explains the apparent non-decay: shrinking `alpha` over
`decay_seconds` has no visible effect on a saturated pixel until `strength * alpha` finally drops
back under `1.0`, so the core reads as static and then vanishes instead of fading.

`explorations/barrier-fx-lab/barrier-fx-lab.html` never had this problem because its `main()` ends
with a single scene-wide tonemap (`outColor = 1.0 - exp(-outColor * uExposure)`, `uExposure`
defaulting to `1.0`) applied *after* every layer (background, barrier, flash) is summed, right
before its own final `clamp(outColor, 0.0, 1.0)` — that step was never ported to the real app, which
has no equivalent single "whole scene" value to tonemap (each pass writes straight into the shared
target). Fixed by applying the same `1.0 - exp(-x)` curve locally, per-draw, inside `fs_glow` itself
— it now maps the resolved `color * strength_rgb * alpha` through that curve before writing, so any
single additive draw softly compresses toward `1.0` instead of hard-clamping (reads as a bright
point, not a disc) and keeps visibly responding to `alpha` (and thus decay) across its whole range.
This is scoped to `effects.wgsl`'s `fs_glow` (flash cores and additive particles) only —
`barrier.wgsl`'s `fs_glow` has the same additive-onto-unorm shape but never dives deep enough
negative to blow up this way, since its `edge_dist` is measured from a thin bar whose interior gets
occluded by the opaque core pass drawn on top, not from a filled ellipse with a genuinely negative
interior.

## Effects instancing moved from vertex-buffer attributes to a storage buffer

The immediately preceding entry's fix (packing `FlameCoronaSpec::flicker_independence` into
`layer_sigma.w`) landed with `effects.wgsl`'s `Instance` struct already sitting at wgpu's hard cap
of 16 vertex-attribute locations (`@location(0)..@location(15)`, counting `Vertex`'s own
`@location(0)`) — every `vec4<f32>` field already had all four components spoken for, so the struct
had no room left to grow without another packing trick, and the file's own doc comments said so
explicitly.

Asked directly whether that ceiling would become a problem, then asked to just remove it rather than
revisit the question later: `EffectInstance`'s per-instance data now lives in a storage buffer
(`effects.wgsl`'s `instances: array<Instance>`, bound at group 1) read by `@builtin(instance_index)`
in `vs_main`, instead of being fed through a second vertex buffer's worth of `@location` attributes.
The quad's own `position` vertex buffer (`@location(0)`, per-vertex, `step_mode: Vertex`) is
unaffected — only the *instance* data moved. This removes the location cap for this pipeline
entirely; future `EffectInstance`/`Instance` fields can be added without any packing tricks.

The one real hazard in this move: WGSL computes storage-buffer struct member offsets from its own
alignment rules (`vec2<f32>` aligns to 8, `vec3<f32>` aligns to 16 but sizes to 12, `vec4<f32>`
aligns to and sizes to 16), which do *not* match how Rust's `#[repr(C)]` packs the same field types
(tightly, with no implicit gaps, since every `[f32; N]` field has Rust-side alignment 4 regardless of
`N`). Porting the struct's field types over as-is (`center: vec2<f32>` followed by
`core_radius: vec4<f32>`, etc.) would have left the WGSL side inserting alignment padding the Rust
side doesn't have — a silent per-field byte-offset mismatch neither `bytemuck` nor wgpu's bind-group
validation checks, so it would have shown up as garbled/nonsensical rendering rather than a build or
validation error. Avoided by widening every field to a full `vec4<f32>` on both sides — including
ones that are conceptually smaller (`center`+`quad_radius` merged into one `center_quad_radius` vec4,
`alpha` and each `color_stop_N` given a vec4 slot with the spare component(s) unused) — since
`vec4<f32>`'s alignment equals its size, consecutive vec4 fields always pack back-to-back with zero
gap on both sides by construction, rather than by manually computed and easily-wrong offsets.

`effects.wgsl`'s `color_stop_0`..`color_stop_4` (and `sample_stops`) stayed hand-unrolled to
`FLASH_GRADIENT_STOPS == 5` named fields rather than becoming a WGSL array — the existing
`FLASH_GRADIENT_STOPS == 5` compile-time assertion in `effects.rs` (previously guarding a
`wgpu::vertex_attr_array!` call that could not loop over a const-generic count) was kept for the same
reason, just re-pointed at the WGSL side, which still can't loop over a const-generic struct field
count either.

## Note activity list / duration-floor bug

The Keyboard tab's active-note list originally used the raw MIDI note end to decide what counted as
"currently playing," but rendered note instances have a duration floor (very short MIDI notes still
render as at least a 0.1-second bar). That mismatch made the editor report "no notes playing" while
the visible bar was still crossing the barrier. Fixed by matching the active window to the same
rendered duration — now the current, permanent behavior.
