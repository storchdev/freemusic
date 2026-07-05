# `.fmstyle.ron` format reference

This is the field-by-field spec for the `.fmstyle.ron` visual style format. `CLAUDE.md`'s
`.fmstyle.ron` milestone section (Phases A–J) is the historical narrative of *why*/*when* each
piece was built; this document is the living contract of what's actually in the schema today. Keep
the two in sync when the schema changes: update this doc in the same session as the code change,
and append a row to the [breaking-change log](#breaking-change-log) below.

Schema lives in `crates/project/src/style.rs`; keep that module's doc comment pointing back here.

## Overview

A `.fmstyle.ron` file is a [RON](https://github.com/ron-rs/ron) document describing a `Style`. It's
loaded via `Style::load(path)` and imported into a running project through the Project tab's
"Import style…" button (`app/src/ui.rs::draw_project_tab`) — there's no other way to attach one to
a project today. An imported style fully overrides the legacy note/barrier "quick control" sliders
for whichever layers it sets; a project with no imported style has its look synthesized from those
sliders via `Style::from_legacy` (see each layer section below for the exact legacy mapping).

`version` is always `1` right now — there is no migration logic yet. Every field on every type in
this schema is `#[serde(default)]`-compatible, so a file written by an older schema version still
parses; new fields simply take their default value. `version` itself exists so a future breaking
change has somewhere to branch on.

Top level:

```ron
(
    version: 1,
    notes: Static((/* NoteLayer */)),
    barrier: Static((/* BarrierLayer */)),
    transition: Static((/* TransitionLayer */)),
    background: Constant((0, 0, 0)),
)
```

`background: ColorBinding` (default `Constant([0, 0, 0])`, i.e. black) is the canvas clear color —
visible behind the video wherever it doesn't fully cover the frame (e.g. a `VideoTransform` crop/
scale leaving letterbox gaps) and behind the note highway above the barrier. Unlike `notes`/
`barrier`/`transition` it's a plain `ColorBinding`, not wrapped in `Timed` — a single canvas-wide
value has no per-note timeline to key against. A project with no imported style gets this from the
Keyboard tab's own "Background" color picker (`Project::background_color`) instead, via
`Style::from_legacy`'s third parameter.

## `Timed<T>`

Every layer (`notes`, `barrier`, `transition`) is wrapped in `Timed<T>`:

```rust
enum Timed<T> {
    Static(T),
    Keyed(Vec<(f64, T)>),  // (time_seconds, value), any order
}
```

`resolve(t)` returns the last key at or before `t`; if `t` precedes every key, it clamps to the
first key rather than erroring. **v1 only ever calls `resolve(0.0)` once**, at the point a style is
consumed by the renderer/legacy-fallback machinery — there is no live mid-song style swapping yet.
A `Keyed` style with, say, a key at `30.0` currently has no effect until that call site is changed
to re-resolve per-frame against transport time; until then, only the key at/before `t=0.0` (i.e.
whichever key sits at the smallest time `<= 0.0`, or the first key if all keys are `> 0.0`) is ever
visible. Practically, ship `Static` unless you're deliberately preparing for a future where
per-frame resolution lands.

## Notes layer (`NoteLayer`)

The falling notes themselves.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `fill` | `Fill` | `Solid(Constant([255,255,255]))` | Note color: solid or a top→bottom vertical gradient. |
| `sheen` | `Option<Sheen>` | `None` | Diagonal specular stripe swept across the fill. |
| `glow` | `Option<Glow>` | `None` | Soft outer halo past the note's silhouette. |
| `roundedness` | `f32` | `1.0` | Corner radius fraction; `0.0` = square, `1.0` = Neothesia's default rounding, up to `3.0` (UI slider range) for a pill shape. No shader-side clamp — pushing far enough that the radius exceeds half the note's shorter dimension can visually distort rather than error. |
| `fall_speed` | `f32` | `400.0` | Pixels/second. Also scales on-screen note *length*, since note quad height is `duration_seconds * fall_speed` — there's no separate length control. |
| `border` | `Option<Border>` | `None` | **Schema-only, no-op** — see [below](#known-schema-onlyno-op-fields). |
| `black_key_fill` | `BlackKeyFill` | `Auto` | How sharp-key notes are colored relative to `fill`. |

```ron
notes: Static((
    fill: VerticalGradient(top: Constant((120, 220, 255)), bottom: Constant((30, 90, 200))),
    sheen: Some((intensity: 0.5, width: 0.8, angle_degrees: 45.0)),
    glow: Some((color: Constant((120, 200, 255)), radius_px: 12.0, brightness: 1.0)),
    roundedness: 1.0,
    fall_speed: 400.0,
    border: None,
    black_key_fill: Auto,
)),
```

### `Fill`

```rust
enum Fill { Solid(ColorBinding), VerticalGradient { top: ColorBinding, bottom: ColorBinding } }
```

`Solid` gives every note one color; `VerticalGradient`'s `top`/`bottom` are independently resolved
`ColorBinding`s (each darkened separately for sharp keys under `BlackKeyFill::Auto`).

### `BlackKeyFill`

```rust
enum BlackKeyFill { Auto, Same, Custom(Fill) }
```

- `Auto` (default): darken the white-key `fill`'s resolved color(s) by `0.6` — today's only
  pre-Phase-F behavior, kept as the default for pixel parity.
- `Same`: no darkening; sharp keys use the exact same fill as natural keys.
- `Custom(Fill)`: an independently resolved fill (solid or gradient) just for sharp keys, unrelated
  to the natural-key `fill`.

Legacy mapping (`Style::from_legacy`, from `NoteStyle::black_key_color: BlackKeyColorMode`):
`Auto`→`Auto`, `Same`→`Same`, `Custom(color)`→`Custom(Fill::Solid(Constant(color)))`.

### `Sheen`

```rust
struct Sheen { intensity: f32, width: f32, angle_degrees: f32 }
```

A brightening band swept diagonally (`angle_degrees`) across each note's fill, `width` fraction of
the note wide, blended in at `intensity`.

### `Glow`

```rust
struct Glow { color: ColorBinding, brightness: f32, layers: [GlowLayer; 3] }
struct GlowLayer { amplitude: f32, sigma_px: f32 }
```

Halo color/heat, plus (since Phase M) a 3-layer additive corona shape. The rendered quad is
inflated by `max(layer sigmas) * 5.0` pixels on every side so there are pixels to paint the corona
onto (an exact no-op when `glow` is `None` — the inflation margin is `0.0`). Shared by
`NoteLayer::glow` and `BarrierLayer::glow` — the same struct drives both.

`brightness` (default `1.0`) is the single knob for how much light the corona adds (and, on the
glowing surface's own opaque fill, still drives the `hot_color` whitening mix) — see
[Brightness/overexposure](#brightnessoverexposure) below. There used to be a separate `intensity`
(halo opacity) field alongside `brightness` (**removed in Phase L**); a single `radius_px` field
that controlled reach (**replaced by `layers` in Phase M** — see the changelog).

`layers` (default: tight/mid/wide — `[(amplitude: 2.6, sigma_px: 5.0), (1.1, 16.0), (0.38, 48.0)]`)
is three `amplitude * exp(-d / sigma_px)` exponential falloff terms, `d` = distance outside the
glowing surface's opaque edge, summed additively into a single light value — this is what makes
the corona read as light genuinely radiating from a white-hot core rather than a single flat
(possibly whitened) color at one spatial scale. `sigma_px` sets how far each layer reaches (bigger
= softer/wider spread), `amplitude` sets how bright that layer is. Unlike Phase K/L's `radius_px`,
`brightness` no longer widens reach at all — reach is purely `layers[i].sigma_px`-driven now.
**RON serializes a fixed-size `[GlowLayer; 3]` array with tuple parens `layers: (...)`, not
brackets `layers: [...]`** — confirmed empirically, easy to get wrong hand-editing a file.

## Barrier layer (`BarrierLayer`)

The horizontal line/bar where falling notes stop.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `color` | `ColorBinding` | `Constant([255,255,255])` | Bar color. |
| `thickness` | `f32` | `4.0` | Bar thickness in **canvas pixels** (not on-screen/logical UI points — depends on preview scale, same coordinate space the falling notes render in). |
| `glow` | `Option<Glow>` | `None` | Soft radiating halo around the bar; presence *is* the on/off switch (**replaced `kind: BarrierKind` + `glow_radius_px: f32` in Phase K** — see the changelog). |
| `pulse` | `Option<Pulse>` | `None` | Brief brighten-then-decay on each note arrival. |
| `wavy` | `Option<WavySpec>` | `None` | Rippling "calm ocean" edge instead of a flat line. Drives off deterministic transport time (`time_seconds` after subtracting sync offset) — so it's frame-reproducible in export and freezes exactly on pause/scrub, not wall-clock/frame-count based. |
| `show_bar` | `bool` | `false` (Phase M) | Whether the flat/opaque bar itself renders at all, independent of `glow` — a style with `glow: Some(..)` and `show_bar: false` renders pure additive corona with no visible opaque bar shape (a single glowing blade, not a colored line with a halo around it). Defaults to `false` since the corona, not the flat bar, is the look this format is designed around; opt in explicitly with `show_bar: true` for the older "visible line" look. Not present on `NoteLayer` — a note without its own fill isn't a sensible look. |

```ron
barrier: Static((
    color: Constant((255, 220, 120)),
    thickness: 6.0,
    glow: Some((
        color: Constant((255, 220, 120)),
        brightness: 1.0,
        layers: (
            (amplitude: 2.6, sigma_px: 5.0),
            (amplitude: 1.1, sigma_px: 16.0),
            (amplitude: 0.38, sigma_px: 48.0),
        ),
    )),
    pulse: Some((decay_seconds: 0.35, brightness: 1.6)),
    wavy: Some((amplitude_px: 6.0, wavelength_px: 220.0, speed: 18.0, mode: Edge)),
    show_bar: true,
)),
```

### `Pulse`

```rust
struct Pulse { decay_seconds: f32, brightness: f32 }
```

Stateless: recomputed every frame from the sorted note-onset list (no spawn/tracking bookkeeping),
so it's correct under scrubbing in either direction with no special-casing.

`brightness` (default `1.6`) is the peak color multiplier applied to the bar's own color at
`pulse = 1.0`, decaying back to the bar's resting brightness (its `Glow::brightness` if it has a
glow, else `1.0`/no-op) as the pulse settles — see
[Brightness/overexposure](#brightnessoverexposure). There used to be a separate `intensity` (0..1
peak amplitude) field that scaled into `brightness` (**removed in Phase L**, redundant with
`brightness` alone) — what used to be `intensity: 0.8, brightness: 1.6` (peak effective multiplier
`1.48`) becomes simply `brightness: 1.48`, or whatever peak look is actually wanted. The default
`1.6` is a tune-by-eye starting point, not derived from anything — adjust per style if it looks too
subtle or too blown-out.

### `WavySpec`

```rust
struct WavySpec { amplitude_px: f32, wavelength_px: f32, speed: f32, mode: WavyMode }
```

- `amplitude_px`: peak vertical displacement in canvas pixels. The waveform is a sum of three
  incommensurate-frequency sine terms weighted 0.6/0.3/0.1 (sums to 1.0), so `|offset| <=
  amplitude_px` always holds exactly — a "calm ocean cross-section," not one obvious repeating
  sine.
- `wavelength_px`: pixels per cycle of the dominant (slowest) term.
- `speed`: how fast the ripple crawls sideways over transport time; `0` freezes the shape in place
  (still x-varying, just not animating).
- `mode` (`WavyMode`, default `TopWave`):
  - `TopWave`: only the top edge ripples, bottom stays flat — bar thickness varies across its
    width, can pinch thin at wave troughs.
  - `Edge`: the identical offset applies to both edges — the whole bar rigidly translates
    (constant thickness), reads as a thin curvy line rather than a bar with volume.
  - `FullWave`: both edges bulge outward together, correlated with the same wave, guaranteeing
    thickness is always `>= thickness` (never pinches below the configured value) while still
    swelling at wave crests.

`None` (the default) means a perfectly flat edge — pixel-identical to pre-Phase-G behavior.

## Transition layer (`TransitionLayer`)

Effects spawned when a note arrives at the barrier.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `kind` | `TransitionKind` | `None` | `None` / `Particles` / `Flash` / `ParticlesAndFlash`. |
| `particles` | `Option<ParticleSpec>` | `None` | Required (non-`None`) when `kind` includes particles. |
| `flash` | `Option<FlashSpec>` | `None` | Required (non-`None`) when `kind` includes a flash. |

```ron
transition: Static((
    kind: ParticlesAndFlash,
    particles: Some((
        count: 24, lifetime_seconds: 0.4, size_px: 4.0, speed_px: 180.0,
        spread_degrees: 60.0, gravity_px: 300.0, color: Constant((255, 240, 200)),
        additive: true, emission: Burst, brightness: 1.0,
        layers: (
            (amplitude: 2.6, sigma_px: 5.0),
            (amplitude: 1.1, sigma_px: 16.0),
            (amplitude: 0.38, sigma_px: 48.0),
        ),
    )),
    flash: Some((
        radius_x_px: 40.0, radius_y_px: 40.0,
        color: Constant((255, 255, 255)), decay_seconds: 0.15, mode: Instant, brightness: 1.0,
        layers: (
            (amplitude: 2.6, sigma_px: 5.0),
            (amplitude: 1.1, sigma_px: 16.0),
            (amplitude: 0.38, sigma_px: 48.0),
        ),
    )),
)),
```

Note: `kind: None` must be written as `` r#None `` in RON, since `None` is a reserved identifier —
this is what the RON serializer emits automatically; write it the same way by hand.

### `ParticleSpec`

| Field | Type | Meaning |
|---|---|---|
| `count` | `u32` | Particles per burst. **`Burst`-only** — ignored under `Continuous` emission. |
| `lifetime_seconds` | `f32` | How long each particle lives before expiring. |
| `size_px` | `f32` | Particle quad size (circular, `[size_px, size_px]`). |
| `speed_px` | `f32` | Initial speed; each particle jitters between `0.5x`–`1.0x` of this. |
| `spread_degrees` | `f32` | Spawn cone width around straight up. |
| `gravity_px` | `f32` | Downward acceleration applied every step after spawn. |
| `color` | `ColorBinding` | Particle color. |
| `additive` | `bool` | Additive blending (bright, overlapping particles glow) vs. premultiplied-alpha (opaque-ish). Decided once per `update()` call from the layer's currently-resolved value — a particle spawned under one style doesn't retroactively update if a *different* style is imported while it's still alive. |
| `emission` | `EmissionMode` | `Burst` (default) or `Continuous { rate_per_second }`. |
| `brightness` | `f32` | Default `1.0`. Baked into `layers[i].amplitude` at spawn time (a plain multiply, not a `hot_color` mix — see [Brightness/overexposure](#brightnessoverexposure)). `1.0` is a no-op. |
| `layers` | `[GlowLayer; 3]` | Default tight/mid/wide (same as `Glow`, Phase M). **Only read when `additive: true`** — non-additive "puff" particles ignore this field entirely and render as a plain hard-edged dot, unaffected by any value here. |

`EmissionMode::Continuous { rate_per_second }`: particles spawn every frame a note is held,
spread across the *width* of its key (not its center point) — reads as the key being "ground
down" rather than sparking once. `count` has no effect in this mode.

### `FlashSpec`

| Field | Type | Meaning |
|---|---|---|
| `radius_x_px` / `radius_y_px` | `f32` | Independent horizontal/vertical radii — set equal for a circular flash, unequal for an ellipse. **Renamed from a single `radius_px` in Phase H** — see the changelog. |
| `color` | `ColorBinding` | Flash color. |
| `decay_seconds` | `f32` | How long the fade-out takes (see `mode` for when the fade *starts*). |
| `mode` | `FlashMode` | `Instant` (default) or `Sustained`. |
| `brightness` | `f32` | Default `1.0`. Baked into `layers[i].amplitude` at spawn time (a plain multiply, not a `hot_color` mix — see [Brightness/overexposure](#brightnessoverexposure)). `1.0` is a no-op. |
| `layers` | `[GlowLayer; 3]` | Default tight/mid/wide (same as `Glow`, Phase M). A flash is always additive, so this is always read (unlike `ParticleSpec::layers`, which non-additive particles ignore). |

A flash always renders additively, so its old separate `intensity` (peak alpha, 0..1) had the
exact same visual effect as `brightness` (both just scale the additive contribution) — **removed
in Phase L** as a redundant axis. A flash is always fully "on" at spawn/at the start of its hold
(see `mode`), fading to 0 over `decay_seconds`.

`FlashMode`:
- `Instant`: decays over `decay_seconds` starting immediately at note-on — a quick pulse.
- `Sustained`: holds at full brightness for the note's entire held duration, only starting to
  decay (over `decay_seconds`) once the note ends. This is the field most likely to be reached for
  first if you want the "glow triggered by a key press" seemusic/Synthesia look — it stops being a
  flash and becomes a sustained glow that tracks note length. A long-held note stays glowing
  throughout; a short note barely shows before decaying.

## Shared leaf types

### `ColorBinding` / `ScalarBinding`

```rust
enum ColorBinding { Constant([u8; 3]), ByVelocity(Ramp), ByPitchClass([[u8; 3]; 12]), ByTrack(Vec<[u8; 3]>) }
enum ScalarBinding { Constant(f32), ByVelocity { low: f32, high: f32 }, ByPitchClass([f32; 12]), ByTrack(Vec<f32>) }
struct Ramp { low: [u8; 3], high: [u8; 3] }
```

Every color-typed field in this schema is a `ColorBinding`, meant to eventually vary per note by
velocity, pitch class, or track. **Only `Constant` actually varies rendering today.** The other
three variants parse and round-trip correctly, but resolve (via `resolve_constant()`) to a fixed
representative value and print a one-time stderr warning the first time this happens per process:

- `ByVelocity(ramp)` → `ramp.high` (the loudest-note color).
- `ByPitchClass(colors)` → `colors[0]`.
- `ByTrack(colors)` → `colors.first()`, or white if the list is empty.

`ScalarBinding` (currently unused by any field in this schema, reserved for a future numeric
per-note property) follows the identical shape and fallback rule (`ByVelocity.high`,
`ByPitchClass[0]`, `ByTrack.first()` or `1.0`).

## Brightness/overexposure

Every "glow" effect in this schema (note glow, barrier glow, barrier pulse, flash, particles) has a
`brightness: f32` knob (default `1.0`). It went through three designs:

**Phase K** (superseded, kept here for history): `brightness` was a plain multiply
(`color * brightness`) applied only to a glow's *halo*, alongside a separate `intensity` knob that
scaled the halo's opacity. The non-HDR `Rgba8Unorm` render target clamps per-channel values above
`1.0` to white on write, so a channel already near saturation clipped first — but multiplying a
color's channels up doesn't converge to *white* unless they already share the same magnitude (e.g.
`[1.0, 0.3, 0.1] * 3.0` clips to `[1.0, 0.9, 0.3]`, a more-saturated orange, not white), and the
halo was the only thing that changed — a bright glow read as a colored ring stuck to the object's
edge, not the object itself heating up.

**Phase L** (superseded, kept here for history): `brightness` was the single knob, `intensity` gone
everywhere it used to coexist with `brightness`. One mechanism, applied consistently to every
glowing surface's *own* color, not just its halo:

> Desaturate the color toward pure white as `brightness` climbs past `1.0`
> (`hot_color(base, brightness) = mix(base, white, 1 - 1/brightness)` for `brightness > 1`,
> `base * brightness` — a plain dimmer — at or below `1.0`), and let the halo's reach grow
> modestly with the same brightness (`corona_reach_scale(brightness) = 1 + 0.5*(1 - 1/brightness)`,
> saturating at `1.5x` the configured `radius_px`).

This made the *core* of a glowing note or the barrier bar itself look white-hot rather than only
the halo around it changing, but the halo itself was still a single flat (possibly whitened) color
alpha-blended at one spatial scale — testing it next to a real reference, this "just looks like a
lighter and more desaturated color," not a glowstick/lightsaber corona.

**Phase M (current)**: real bloom is additive — light from multiple spatial scales *adds* onto the
background and only clamps to white where the sum saturates, rather than one flat color painted
over it. Split into two parts, applied consistently across barrier glow, note glow, particles, and
flashes:

- **Opaque surfaces** (a note's own fill, the barrier's own flat bar) keep Phase L's `hot_color`
  whitening mix exactly as described above — this part was already right, just scoped down to
  *only* opaque geometry, no longer also standing in for the halo.
- **Light/corona** (the halo around a note or barrier, particle sparks, flashes) is additive:
  `light = color * Σ_{i=1..3}(layers[i].amplitude * exp(-d / layers[i].sigma_px)) * brightness`,
  blended with `wgpu::BlendState { src: ONE, dst: ONE }` (`ONE`/`ONE`, additive) instead of alpha
  blending. `d` is always "distance outside the opaque core's edge, 0 inside" — the same distance
  field each renderer already computed for the old smoothstep-based halo. Whitening happens for
  free via additive saturation — no explicit mix-toward-white needed for the corona.
  `brightness = 1.0` is an exact no-op (as before), but `brightness` **no longer widens reach** —
  Phase L's `corona_reach_scale` is gone; reach is purely `layers[i].sigma_px`-driven now, a
  deliberate simplification.

Because an opaque core (alpha-blended, occludes what's behind it) and an additive halo (adds to
what's behind it) can't share one `wgpu::BlendState`, the barrier and notes renderers each run
**two render pipelines** — an additive glow pass drawn first, then the existing alpha-blended core
pass drawn second so the opaque core correctly occludes the glow directly beneath it. Particles/
flashes don't need this split: nothing stacks on top of them the way video/notes stack on top of
the barrier/note glow, so "hard dot + additive halo" can share one draw — no dual-pipeline
plumbing there, just the formula.

Where it lives per effect:

- **Note glow / barrier glow** (`Glow::layers`/`Glow::brightness`): drives both the new
  `fs_glow` additive pass (halo) and the existing `hot_color` mix on the surface's own opaque fill
  (`notes/shader.wgsl`/`barrier.wgsl`).
- **Barrier pulse** (`Pulse::brightness`): unchanged in mechanism — the peak brightness fed into
  the same `hot_color` mix at `pulse = 1.0`, decaying back to the bar's resting brightness (its
  `Glow::brightness`, or `1.0`) as the pulse settles; this value also feeds the corona's brightness
  multiplier during the pulse.
- **Flash / particles** (`FlashSpec::layers` / `ParticleSpec::layers`, both additive-only paths):
  bake `layers[i].amplitude * brightness` into a GPU instance before upload (a plain multiply, not
  a `hot_color` mix — there's no separate opaque core to whiten here), then run the identical
  additive layered-sum formula in `effects.wgsl`'s `fs_glow`. Non-additive "puff" particles are
  unaffected — `layers` is simply not read on that path.

## Known schema-only/no-op fields

One place to check "why doesn't this do anything" before assuming it's a bug:

- **`NoteLayer::border` (`Border`)**: `struct Border { color: ColorBinding, width_px: f32 }` parses
  and round-trips but no renderer draws it.
- **Any non-`Constant` `ColorBinding`/`ScalarBinding`** (`ByVelocity`/`ByPitchClass`/`ByTrack`):
  parse and round-trip, resolve to a fixed fallback constant as described above, and are not yet
  driven by real per-note velocity/pitch/track data.

## Breaking-change log

| Phase | Change |
|---|---|
| A | Initial schema: `Style { version, notes, barrier, transition }`, `Timed<T>`, `ColorBinding`/`ScalarBinding`, `NoteLayer` (`fill`/`sheen`/`glow`/`roundedness`/`fall_speed`/`border`), `BarrierLayer` (`kind`/`color`/`thickness`/`glow_radius_px`/`pulse`), `TransitionLayer` (`kind`/`particles`/`flash`). |
| F | `NoteLayer` gained `black_key_fill: BlackKeyFill` (new enum: `Auto`/`Same`/`Custom(Fill)`). Additive — old files still parse, default to `Auto`. |
| G | `BarrierLayer` gained `wavy: Option<WavySpec>` (new `WavySpec`/`WavyMode`). Additive — default `None` (flat edge). |
| H | **Breaking**: `FlashSpec.radius_px: f32` renamed to `radius_x_px: f32, radius_y_px: f32`. Pre-1.0 format, no back-compat shim — an old file with `radius_px` will fail to parse and needs manual editing to the two new fields (set both equal to the old value for an unchanged circular look). Also, internally (not schema-visible): `render::notes`'s `HitEvent` type was replaced by `NoteInterval` in Phase I, not Phase H — see below. |
| I | `ParticleSpec` gained `emission: EmissionMode` (new enum: `Burst`/`Continuous { rate_per_second }`). `FlashSpec` gained `mode: FlashMode` (new enum: `Instant`/`Sustained`). Both additive — old files still parse, default to `Burst`/`Instant` (pixel-identical to pre-Phase-I behavior). Internal-only (not part of the RON schema): `render::notes::HitEvent { time_seconds, x_px }` was replaced by a richer `NoteInterval { start_seconds, end_seconds, x_left, x_right }` to support continuous emission's key-width spread — this only matters if you're modifying the renderer, not authoring `.fmstyle.ron` files. |
| J | Documentation only (this file) — no schema change. |
| K | **Breaking**: `BarrierLayer` dropped `kind: BarrierKind` + `glow_radius_px: f32`, gained `glow: Option<Glow>` (the same `Glow` struct `NoteLayer` already used) — presence of `Some(Glow{..})` now *is* the on/off switch, replacing the enum. An old file with `kind`/`glow_radius_px` needs manual editing: `kind: Line` → `glow: None`; `kind: Glow, glow_radius_px: R` → `glow: Some((color: <old barrier color>, radius_px: R, intensity: 1.0, brightness: 1.0))` (reproduces the pre-Phase-K look exactly — Phase K's own `intensity` field was subsequently removed in Phase L, see below). Additive, not breaking: `Glow` gained `brightness: f32` (default `1.0`); `Pulse` gained `brightness: f32` (default `1.6`); `FlashSpec`/`ParticleSpec` each gained `brightness: f32` (default `1.0`) — old files without these fields still parse via `serde(default)`. |
| L | **Breaking**: `intensity: f32` removed from `Glow`, `Pulse`, and `FlashSpec` (redundant once `brightness` alone drives the whole look — see [Brightness/overexposure](#brightnessoverexposure)). An old file needs manual editing: drop the `intensity` line from any `Glow`/`FlashSpec` literal; for `Pulse`, fold `intensity`/`brightness` into a single `brightness` equal to their product (e.g. `intensity: 0.8, brightness: 1.6` → `brightness: 1.28`, adjust to taste). Also, not schema-visible: `Glow`/`Pulse`/barrier's and notes' own core/fill color now go through a shared `hot_color` whitening function and the halo falloff changed from a flat-opacity smoothstep band to a natural radiating `pow` curve — see [Brightness/overexposure](#brightnessoverexposure) for the full mechanism. |
| M | **Breaking**: `Glow.radius_px: f32` removed, replaced by `layers: [GlowLayer; 3]` (new struct `GlowLayer { amplitude: f32, sigma_px: f32 }`) — the halo is now an additive multi-layer corona instead of a single alpha-blended ring, see [Brightness/overexposure](#brightnessoverexposure). An old file needs manual editing: drop the `radius_px` line from any `Glow` literal and add a `layers: (...)` tuple (the default tight/mid/wide set — `(amplitude: 2.6, sigma_px: 5.0), (amplitude: 1.1, sigma_px: 16.0), (amplitude: 0.38, sigma_px: 48.0)` — is a reasonable starting point; scale sigmas roughly proportional to the old `radius_px` for a similar reach). Additive, not breaking: `FlashSpec`/`ParticleSpec` each gained the same `layers: [GlowLayer; 3]` field (default as above; ignored on non-additive particles); `BarrierLayer` gained `show_bar: bool` (default `false` — an old file with no `show_bar` now renders pure corona with no visible opaque bar unless it opts in with `show_bar: true`). |
| N | Additive, not breaking: `Style` gained `background: ColorBinding` (default `Constant([0, 0, 0])`, i.e. black — matches the hardcoded clear color every renderer used before this field existed, so old files render unchanged). Also new: `Project::background_color: [u8; 3]` (the legacy/no-imported-style equivalent, edited via the Keyboard tab's "Background" color picker) and `Project::effective_background_color()`/`Style::from_legacy`'s new third parameter, mirroring the existing `effective_note_layer`/`effective_barrier_layer`/`effective_transition_layer` pattern. |

If a previously-working `.fmstyle.ron` file fails to load after an upgrade, check this table first
— Phase H's rename, Phase K's `BarrierLayer` rework, Phase L's `intensity` removal, and Phase M's
`radius_px` → `layers` rework are the schema-breaking changes so far (Phase N is additive, no file
needs editing because of it).
