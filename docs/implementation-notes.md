# Implementation notes

These notes hold historical debugging context and design rationale that is useful to preserve but
too bulky for inline code comments. Code comments should keep the current invariant close to the
code and point here when the older failure mode matters.

## Video decode timing

`video-pipeline::default_decode_threads` caps decode workers at four logical CPUs, even on larger
machines. The cap fixed playback lag during mouse movement on hybrid CPUs: four resident workers
stayed on performance cores instead of oversubscribing and spilling onto slower efficiency cores,
while still decoding typical footage in real time. `FREEMUSIC_DECODE_THREADS` can override it.

`VideoPipeline::seek_and_decode(..., exact)` separates explicit scrubs from ordinary playback ticks
that happen to require a reseek after a redraw-cadence stall. A backward jump or a large forward
jump triggers a real seek. For explicit timeline dragging, `exact = false` can return the first
post-seek frame near the preceding keyframe because the user is still adjusting the position. For
export and normal playback, `exact = true` decodes forward until the frame timestamp reaches the
target. This avoids the old failure mode where a stalled redraw reseek landed on an old keyframe,
the next redraw saw the same gap, and playback appeared to loop over the same handful of frames.

## Preview color management

The interactive preview renders into an `Rgba8Unorm` offscreen texture because
`egui_wgpu::Renderer::register_native_texture` requires that format. Decoded video is sampled from
`Bgra8UnormSrgb`, so sampling yields linear values. An sRGB render target would encode those values
on store, but a plain `Unorm` target stores them as-is. Without the shader's manual sRGB encode,
midtone bytes are later interpreted as gamma-encoded and the preview looks darker than players such
as mpv. Export renders straight to `Bgra8UnormSrgb`, so it leaves manual encode disabled.

## Barrier and transition glow history

The barrier renderer began as an egui overlay and moved into a wgpu pass so the same compositor
could render both the interactive preview and exported video. The current pass is a full-canvas
quad with an optional opaque core and optional glow.

The "white-hot pipe" redesign removed separate glow and pulse intensity knobs. Brightness now
drives the core color too, desaturating toward white above `1.0`; this keeps a bright barrier from
reading as a flat bar with a disconnected colored ring around it.

The additive corona redesign replaced a single alpha-blended halo with a sum of three exponential
falloff layers. Barrier and note glow use separate additive and opaque passes: additive light draws
first, then the opaque core draws over it so the core can occlude glow beneath its footprint.
`show_bar` independently controls the core, so a barrier can be pure glow, pure bar, both, or
neither.

Transition particles and flashes use the same additive corona math for additive-mode effects.
They do not need the barrier's opaque-core split: additive effects never need to occlude geometry
under themselves. Non-additive puff particles keep the premultiplied-alpha path and hard-edged
look.

Transition effects are stateful, unlike the barrier pulse. Particle position depends on elapsed
time, velocity, gravity, spawn time, and RNG state, so it cannot be recomputed from transport time
alone. The update loop tracks the previous transport time, advances the pool by the delta, spawns
bursts for note arrivals crossed since the last update, and clears the pool on large jumps because
there is no single correct mid-scrub transient state.

## Note activity and layout edge cases

The keyboard tab's active-note list uses the same duration floor as rendered note instances. Very
short MIDI notes still render as at least a 0.1-second bar, so using the raw MIDI note end made the
editor report "no notes playing" while the visible bar was still crossing the barrier. The active
window now matches the rendered duration.

`piano_layout::KeyboardLayout::from_range` has an edge case when asked for a truncated range that
starts mid-octave. Segment layout avoids that by querying from the segment start through the real
keyboard end, then discarding trailing keys with `take(segment_len)`. That makes each segment query
a suffix of the known-good standard 88-key layout and avoids the mid-octave truncation panic.

## Verification helpers

`scripts/gen-test-video.sh` defaults to a 30-second clip with a visible per-frame counter and a
multi-second keyframe interval. That pattern caught the reseek-every-frame and seek timestamp unit
bugs; a static test card would not show whether frames were actually advancing. The longer default
also avoids a common false negative when testing with real MIDI files whose first note may begin
well after ten seconds.

`scripts/run-app.sh` forces the X11 backend under WSLg/Hyprland so `xdotool` and `import` can see
the window. It intentionally runs the app in the foreground of the script; callers that need a
background app should background the script invocation itself, not add `&` or `disown` inside it.

## Static build troubleshooting history

`docs/building.md` has the current build instructions. These are the historical failure modes that
shaped the scripts and vendored FFmpeg patches.

Static release builds must use a static-only `libx264` built from source and surfaced through
`PKG_CONFIG_PATH`. Installing a system/Homebrew/apt `libx264` is unsafe for this project: if a
shared `libx264.so`/dylib appears on the linker's default search path, the linker can silently
prefer it over the explicit static archive. The build then looks "static" from cargo's point of
view while still depending on a shared x264 library at runtime. The helper scripts force
`-l static=x264` and the Linux script runs `ldd` as a final sanity check.

The published `ffmpeg-sys-next 8.1.0` build script passed GCC-only `-march=native -mtune=native`
flags to FFmpeg's `configure` even under MSVC. `cl.exe` rejects those flags, and FFmpeg reported
the failure as the much less specific "C compiler test failed." The vendored
`vendor/ffmpeg-sys-next/` patch skips those flags for `target_env = "msvc"`.

The same build script also parsed MSVC `-libpath:DIR` flags as `-l` library-name flags because both
start with `-l`. That produced invalid `cargo:rustc-link-lib=ibpath:...` output and rustc E0459.
The vendored patch detects MSVC libpath flags case-insensitively and routes them into the
link-search path handling instead.

Static x264 must match rustc's target architecture, not just whichever Visual Studio shell happens
to be open. Early Windows attempts reused a cached x86 x264 archive for x64 builds and later built
x264 from an x86 Developer Shell while cargo still targeted `x86_64-pc-windows-msvc`. The current
Windows script derives the required architecture from `rustc -vV`, keys the x264 cache by that
architecture, and fails fast if the active MSVC environment does not match.

The generic "Developer PowerShell for VS 2022" shortcut can start a 32-bit PowerShell host and
therefore default to an x86 developer environment. `scripts/setup-msvc-x64.ps1` avoids that by
calling `vcvars64.bat` directly.

Running `vcvars64.bat` directly from PowerShell sets environment variables only inside a child
`cmd.exe`, so the caller loses `PATH`, `INCLUDE`, `LIB`, and related variables as soon as the batch
file exits. `setup-msvc-x64.ps1` runs `vcvars64.bat` under `cmd /c "... && set"`, captures the
resulting environment, and applies it to the current PowerShell process.

Windows PowerShell 5.1 reads BOM-less scripts through the system codepage rather than UTF-8. The
PowerShell scripts are saved with a UTF-8 BOM so comments containing non-ASCII punctuation do not
corrupt tokenization under PS 5.1; preserve that BOM when editing them.
