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
