# `.fmstyle.ron` history

This file holds design history, migration notes, and bug-fix context for the style format. The
current field-by-field contract lives in `docs/fmstyle-format.md`.

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

## Breaking-change log

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
