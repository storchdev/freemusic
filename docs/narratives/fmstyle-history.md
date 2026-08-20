# `.fmstyle.ron` history (narrative)

This file holds design history, migration notes, and bug-fix context for the style format —
decision history and worked/didn't-work narrative, not the current-state contract. The current
field-by-field contract lives in `docs/fmstyle-format.md`. Phase-by-phase development narrative
for the format lives alongside this file in `docs/narratives/fmstyle-milestone.md`.

## Black-key gradient bug

`BlackKeyFill::Custom(Fill::VerticalGradient { .. })` used to drop its `bottom` color whenever the
natural-key `fill` was `Solid`; sharp-key notes rendered flat at the gradient's `top` color. The
root cause was a style-wide `fill_kind` uniform derived only from the natural-key fill. The shader
now always blends each note's baked `color_top`/`color_bottom`; for solid fills those two colors
are equal, so the blend is an exact no-op. The old uniform slot is documented in
`crates/render/src/notes/pipeline.rs`.

An initial fix changed WGSL `var color` to `let color`, but later sheen code still mutates it.
`cargo build` and `cargo clippy` did not catch this because embedded WGSL is validated only when a
shader module is created. Future WGSL edits need an app run or shader-module smoke test.

## Glow and brightness design

The glow system went through three designs.

Phase K treated `brightness` as a plain multiplier on halo color, alongside a separate `intensity`
opacity knob. On non-HDR `Rgba8Unorm` targets, channel values above `1.0` clamp, but multiplying an
orange color does not converge cleanly to white; it usually becomes a harsher saturated orange.
Because only the halo changed, bright glows read as colored rings rather than heated objects.

Phase L removed `intensity` where it overlapped with `brightness` and introduced `hot_color`: for
`brightness > 1.0`, the opaque surface color desaturates toward white; at or below `1.0`, it acts
as a dimmer. This made glowing surfaces look white-hot, but the halo was still one flat
alpha-blended shape.

Phase M replaced the single halo with an additive three-layer corona:

```text
light = color * sum(layer.amplitude * exp(-distance / layer.sigma_px)) * brightness
```

The additive light pass uses `ONE`/`ONE` blending. Barrier and note renderers draw an additive glow
pass first and an opaque core pass second so the core can occlude glow beneath it. Particles and
flashes do not need that split because they do not have a separate opaque surface over the glow.

Notes briefly shared the barrier's white-hot fill behavior, but whitening the note's own fill read
as an artifact. Notes now blend a thin rim toward the corona's own edge color/brightness instead.
An earlier rim implementation used `color * brightness`, which could be dimmer than the actual
corona when layer amplitudes summed above `1.0`; the current rim matches the corona contribution
at `distance == 0`.

## `MatchNote` color sampling: the retracted per-pixel sheen sample

An earlier version of `ParticleColor`/`FlashColor::MatchNote` sampled several points *across* a
note's leading edge, specifically to reproduce a diagonal `Sheen` stripe's horizontal brightening
band (the only thing that varies a note's color left-to-right — plain
`Fill::Solid`/`VerticalGradient`/`CanvasGradient` are all uniform across a note's width, only ever
varying color top-to-bottom or by canvas height). That was retracted: it meant hand-porting
`shader.wgsl`'s fill/sheen math into Rust (`render::notes::mod.rs`, since removed), which only
stayed correct for sheen specifically — any *other* future note-color effect (a different
stripe/pattern, a texture, anything not already mirrored in that Rust port) would silently be
invisible to `MatchNote` while still rendering correctly on the note itself, a maintenance trap
that would only get worse as note styles grow. The current design — resolving only the
`(color_top, color_bottom)` pair every `Fill` variant produces by construction
(`resolve_fill_base`'s contract) — doesn't have this problem: every `Fill` variant, current or
future, resolves to that pair, so `MatchNote` stays correct with zero additional code for anything
built on top of that contract. The tradeoff is giving up the sheen-driven cross-section fidelity in
exchange for that guarantee.

## God rays: wander tried and rejected

`GodRaySpec`'s beams were briefly given angular *wander* (the whole beam pattern drifting side to
side over time), ported from the `barrier-fx-lab` exploration that originated the god-ray effect.
It read as the beams wiggling rather than radiating from a fixed sun, so it was removed. The
current design's `rotation_speed_deg_per_sec` (a rigid whole-pattern spin) is a deliberately
different and subtler motion kept as an escape hatch, not a reintroduction of wander.

## Flash corona: the flat-plateau bug, and adding turbulence

A user comparing a rendered flash against a real photograph (a Rousseau reference frame with a
volumetric light burst) reported two things: the corona's bright center read as a "solid" disc
with a jarring hard edge even at a tiny radius (`radius_x_px`/`radius_y_px: 3.0`), and the whole
light — both the corona and the god rays — looked too smooth/glassy compared to the rough,
grainy texture of a real photographed light source.

The first turned out to be a real bug in `effects.wgsl`'s `core_strength` (shared by flash and
additive-particle coronas): the ellipse-aware falloff distance was `select(0.0, edge_dist_px, norm
> 1.0)` — clamped to exactly `0.0` for every pixel *inside* `core_radius`, so the three glow layers
summed to one flat constant across the entire interior (for `photoreal-sunburst.fmstyle.ron`'s
flash, `~2.7`, comfortably clipping to solid white on an `Rgba8Unorm` target) regardless of how
close to the center a pixel actually was. Only outside the ellipse did the exponential falloff
apply at all, producing a visible slope discontinuity right at the boundary — a flat disc glued to
a soft halo, not a continuous light source. The fix removes the clamp entirely: `edge_dist_px` is
now a genuinely *signed* distance (negative inside the ellipse), so the same exponential curve that
shapes the outer halo continues inward and peaks at the center instead of flattening out. This
changes the *rendered look* of every existing flash/additive-particle style without touching the
`.fmstyle.ron` schema at all — a pure shader-logic fix, not a format change. (An even earlier
`(norm - 1.0) * min(core_radius.x, core_radius.y)` formula predates this fix and has its own
comment in `core_strength` explaining why it underestimated distance for elongated ellipses; that
issue is orthogonal to the plateau bug and the current formula still avoids it.)

The second (roughness) genuinely needed new configuration, since nothing in the schema drove any
kind of spatial noise across the corona/god-ray shape itself — `GodRaySpec`'s existing streak/
flicker noise only varies *along* a beam's length, not the light's overall silhouette. The design
question was where to put the new knob: per-effect (separate turbulence controls on the core corona
vs. `GodRaySpec`) or one field applied uniformly to the whole light stack. Asked directly, the user
picked the single shared field — `FlashSpec::turbulence: Option<TurbulenceSpec>` — reasoning that a
real photographed light doesn't have independently-turbulent "corona" and "god ray" components, it's
one light source. Mechanically this is a domain warp: `effects.wgsl`'s `domain_warp` displaces the
fragment's sample point through a 2D value-noise field (the same `hash21`/`noise2` construction
already used elsewhere) before `core_strength`/`god_ray_strength`/`ring_strength` all run, applied
once in `total_strength` so every consumer of the light stack (including each chromatic-aberration
channel sample) sees the same warped shape.

The first attempt gave `turbulence` its own dedicated `@location(16)` vertex attribute on
`effects.wgsl`'s `Instance` struct, mirroring how `godray_a`/`b`/`c`/`ring_chromatic` were added in
Phase V. That crashed at pipeline creation (`wgpu error: ... vertex attribute location 16 must be
less than limit 16`) the first time the user actually ran the app — wgpu caps a pipeline at 16
vertex attribute locations *total*, counting `Vertex`'s own `@location(0)` alongside every
`Instance` field, and this pipeline's existing fields (`center` through `ring_chromatic`) already
used exactly locations 0-15 before turbulence needed anywhere to go. `cargo build`/`clippy` can't
catch this — like the black-key gradient bug above, invalid WGSL/pipeline state is only validated
when the app actually creates the render pipeline at startup, which is also why the "never run the
app yourself" rule in `CLAUDE.md` means this class of bug can only be caught by asking the user to
run it. The fix packs `turbulence`'s three floats into already-declared fields' otherwise-unused
trailing components instead of adding a 17th location: `core_radius` widened from `vec2` to `vec4`
(`.zw` = `strength_px`/`scale_px`) and `layer_amp` widened from `vec3` to `vec4` (`.w` = `speed`).
This only touches the vertex-buffer-supplied `Instance` struct (the thing wgpu's 16-location limit
actually constrains) — `VertexOutput` (the vertex-to-fragment interstage struct) has a much higher
component budget and was never at risk, so it kept its own separate `core_radius: vec2`/
`layer_amp: vec3`/`turbulence: vec4` fields unchanged, with `vs_main` doing the unpack/repack in
between.

## Lab cleanup: god rays/turbulence/electric wisps removed, flame corona fixed

By the time the aurora-corona flame-stack experiment (`flameCoronaStrength` in
`barrier-fx-lab.html`) had settled into a look the user liked, three other lab-only groups had
become dead weight: god rays and turbulence/grain were superseded by the flame corona as the
lab's directional-flash focus (and god rays/ring/chromatic-aberration were already shipped in the
real app per the previous section — the lab copies were purely reference at that point), and the
electric sliding-filament/wisp groups were unused, never-ported experiments predating the flash
work entirely. All three were deleted outright from the lab — schema entries, GLSL uniforms and
functions, `PRESETS` entries, and the three "photoreal sunburst" A/B presets that existed
specifically to compare turbulence modes on god rays (with nothing left to compare once both were
gone). `coreLegacyPlateau` (the flat-plateau-fix A/B toggle, unrelated to turbulence but previously
grouped with it) moved into the "flash" group rather than being deleted, since it's still a useful
A/B against the shipped `core_strength` fix.

Two bugs were found in the flame corona itself along the way, both from feeding raw `atan2` angle
into `noise2`/`fbm`. First: `fbm(vec2(theta * uFlameLobes, ...))` has a hard seam at `theta = ±PI`
(pointing left) regardless of `uFlameLobes` — `atan2` jumps by `2*PI` right there, so the noise
coordinate lands on an uncorrelated patch of the field no matter how the lobe count is tuned, since
tuning it only changes the *size* of the jump, never removes it. The fix samples noise from a point
on a circle parametrized by the light's own unit direction vector instead
(`angularNoisePoint(dir, freq) = dir * freq`, where `dir = offset / r`): since `dir` is just
`(x, y) / r`, a continuous function of position with no branch, sweeping it all the way around has
no discontinuity anywhere, while still tracing the same circumference (and so roughly the same bump
density) the old `theta * freq` coordinate spanned. Second: the streak texture's `r / scale -
time * speed` term made the internal texture visibly flow *outward* from the light's center over
time, which read as material streaming out rather than a light source's brightness varying — the
user's own description was "the outward divergence doesn't look natural." The fix drops the time
term from the streak sampling entirely (now a time-static weave of the same seamless angular
coordinate against radius) and adds a separate whole-corona brightness pulse instead
(`uFlameFlickerSpeed`/`uFlameFlickerIntensity`, same noise-driven flicker shape already used by
`flashContribution`/`flashGodRayStrength`'s equivalents before those were removed), so the corona
now gutters like a real light source rather than flowing.

## Breaking-change log

This is the canonical historical record of every schema-breaking phase; `docs/fmstyle-format.md`
points here rather than repeating the "what the old shape was" detail inline.

| Phase | Change |
|---|---|
| A | Initial schema: `Style { version, notes, barrier, transition }`, `Timed<T>`, `ColorBinding`/`ScalarBinding`, `NoteLayer` (`fill`/`sheen`/`glow`/`roundedness`/`fall_speed`/`border`), `BarrierLayer` (`kind`/`color`/`thickness`/`glow_radius_px`/`pulse`), `TransitionLayer` (`kind`/`particles`/`flash`). |
| F | `NoteLayer` gained `black_key_fill: BlackKeyFill` (`Auto`/`Same`/`Custom(Fill)`). Additive; old files parse with `Auto`. |
| G | `BarrierLayer` gained `wavy: Option<WavySpec>` (`WavySpec`/`WavyMode`). Additive; old files keep a flat edge. |
| H | Breaking: `FlashSpec.radius_px` became `radius_x_px` and `radius_y_px`. Old files need both new fields, usually both set to the old radius. |
| I | `ParticleSpec` gained `emission: EmissionMode`; `FlashSpec` gained `mode: FlashMode`. Both are additive and default to the old burst/instant behavior. Internally, `HitEvent` became `NoteInterval` to support continuous emission across a key width. |
| J | Documentation only. |
| K | Breaking: `BarrierLayer` dropped `kind: BarrierKind` and `glow_radius_px`, and gained `glow: Option<Glow>`. `None` means no glow; `Some(Glow)` is the on/off switch. `Glow`, `Pulse`, `FlashSpec`, and `ParticleSpec` gained `brightness` defaults. |
| L | Breaking: `intensity` was removed from `Glow`, `Pulse`, and `FlashSpec`. Drop it from `Glow`/`FlashSpec`; for `Pulse`, fold it into `brightness` if preserving the old peak is important. |
| M | Breaking: `Glow.radius_px` was replaced by `layers: [GlowLayer; 3]`. `FlashSpec` and `ParticleSpec` also gained `layers`. `BarrierLayer` gained `show_bar: bool`, defaulting to `false`. |
| N | `Style` gained `background: ColorBinding`, defaulting to black. `Project` gained `background_color` for the legacy/no-imported-style path. |
| O | `WavySpec` gained `strands: Option<StrandSpec>` (`StrandSpec`, only meaningful when `mode` is `Edge`, requires `BarrierLayer::glow` to be `Some(..)` to render) and `slide_speed: f32`. Both additive/`#[serde(default)]`; old files render an unchanged flat/still edge. |
| P | `Fill` gained a third variant, `CanvasGradient { top: ColorBinding, bottom: ColorBinding }` — same shape as `VerticalGradient`, but blended across the canvas's own Y position (top of frame -> barrier line) instead of each note's own local height. Additive; old files (which can only ever construct `Solid`/`VerticalGradient`) are unaffected. |
| Q | Breaking: `ParticleSpec.color: ColorBinding` became `color: ParticleColor` (`Fixed`/`MatchNote`/`YGradient`); wrap an existing `Constant(...)` etc. value as `Fixed(...)`. `FlashSpec.color: ColorBinding` became `color: FlashColor` (`Solid`/`HorizontalGradient`/`MatchNote`); wrap as `Solid(...)`. `Glow` gained `match_note_color: bool`, additive/defaulting to `false`. |
| R | Breaking: `ParticleSpec.brightness: f32` and `FlashSpec.brightness: f32` became `ScalarBinding`; wrap an existing bare float as `Constant(...)`, e.g. `brightness: 1.0` -> `brightness: Constant(1.0)`. `Glow.brightness`/`Pulse.brightness` are unaffected, still a plain `f32`. Non-breaking in the same phase: `ColorBinding` gained `resolve_for_note`, so `ByVelocity`/`ByPitchClass`/`ByTrack` now really vary per note instead of resolving to one fixed representative color. |

The schema-breaking phases so far are H, K, L, M, Q, and R.

Also breaking, not tied to a lettered phase above: `ParticleColor::MatchNoteBottom` was renamed to
`MatchNote`, and `FlashColor::MatchNoteBottom` was renamed to `MatchNote` (rename only, no other
field shape change — see `docs/fmstyle-format.md`'s "Note color sampling at the barrier" section
for why "bottom" no longer describes what these sample).

Also breaking, a follow-up in the same session as Phase R: `ParticleSpec.lifetime_seconds`/
`size_px`/`speed_px`/`spread_degrees`/`gravity_px: f32` and `FlashSpec.radius_x_px`/
`radius_y_px`/`decay_seconds: f32` all became `ScalarBinding` (same wrap-in-`Constant(...)` fix as
Phase R; `ParticleSpec.count` stays a plain `u32`, not converted).

Also breaking, from the session that added in-app editing for the whole schema (the Style tab —
see `docs/ui.md`): `project::Project.style` changed from `Option<Style>` to a plain, always-present
`Style` (`#[serde(default = "default_project_style")]`). `NoteStyle`, `BarrierStyle`,
`BlackKeyColorMode`, and `Style::from_legacy` were deleted outright — before this, a project with
no imported style had its look synthesized on the fly from those three legacy "quick control"
types (edited by the old Keyboard tab's Barrier/Note style/Background sliders); after, the Style
tab edits `Style` directly and there's no second, lower-capability representation of a look left to
synthesize from. This was a deliberate design choice, not an oversight: once the Style tab could
edit the *entire* schema, keeping the legacy slider system around as a fallback would have meant
maintaining two ways to represent the same look indefinitely, for no remaining benefit — per
`CLAUDE.md`'s pre-1.0 policy, the cleaner cut was preferred over a compatibility shim. A
`.fmproj.ron` file predating this that has `barrier_style`/`note_style`/`background_color` fields
still loads (unrecognized fields are ignored, not errors), but silently drops to
`default_project_style()`'s look instead of reconstructing the old sliders' values — hand-add an
equivalent `style: (...)` block (see the `NoteLayer`/`BarrierLayer` sections above) to any such
file to preserve its old look. No project files in the repository itself needed this migration.
