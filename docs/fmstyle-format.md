# `.fmstyle.ron` format reference

This is the field-by-field spec for the `.fmstyle.ron` visual style format. This document is the
living contract of what's actually in the schema today; design history and migration notes live in
`docs/fmstyle-history.md`. Keep both in sync when the schema changes.

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
    glow: Some((
        color: Constant((120, 200, 255)),
        brightness: 0.8,
        layers: (
            (amplitude: 2.6, sigma_px: 2.0),
            (amplitude: 1.1, sigma_px: 4.0),
            (amplitude: 0.38, sigma_px: 10.0),
        ),
        edge_blend_px: 6.0,
    )),
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

- `Auto` (default): darken the white-key `fill`'s resolved color(s) by `0.6`.
- `Same`: no darkening; sharp keys use the exact same fill as natural keys.
- `Custom(Fill)`: an independently resolved fill (solid or gradient) just for sharp keys, unrelated
  to the natural-key `fill`.

Legacy mapping (`Style::from_legacy`, from `NoteStyle::black_key_color: BlackKeyColorMode`):
`Auto`->`Auto`, `Same`->`Same`, `Custom(color)`->`Custom(Fill::Solid(Constant(color)))`.

### `Sheen`

```rust
struct Sheen { intensity: f32, width: f32, angle_degrees: f32 }
```

A brightening band swept diagonally (`angle_degrees`) across each note's fill, `width` fraction of
the note wide, blended in at `intensity`.

### `Glow`

```rust
struct Glow { color: ColorBinding, brightness: f32, layers: [GlowLayer; 3], edge_blend_px: f32 }
struct GlowLayer { amplitude: f32, sigma_px: f32 }
```

Halo color/heat, plus a 3-layer additive corona shape. The rendered quad is
inflated by `max(layer sigmas) * 5.0` pixels on every side so there are pixels to paint the corona
onto (an exact no-op when `glow` is `None` — the inflation margin is `0.0`). Shared by
`NoteLayer::glow` and `BarrierLayer::glow` — the same struct drives both.

`brightness` (default `1.0`) is the single knob for how much light the corona adds — see
[Brightness/overexposure](#brightnessoverexposure) below. For the barrier's own opaque bar
(`BarrierLayer::glow`) it also still drives a `hot_color` desaturate-toward-white mix on the bar
itself. Notes with `glow: Some(..)` blend a thin rim near their edge toward the corona's own
color/brightness, not white. This is not a separate toggle.

`layers` (default: tight/mid/wide — `[(amplitude: 2.6, sigma_px: 5.0), (1.1, 16.0), (0.38, 48.0)]`)
is three `amplitude * exp(-d / sigma_px)` exponential falloff terms, `d` = distance outside the
glowing surface's opaque edge, summed additively into a single light value — this is what makes
the corona read as light genuinely radiating from a bright core rather than a single flat
(possibly whitened) color at one spatial scale. `sigma_px` sets how far each layer reaches (bigger
= softer/wider spread), `amplitude` sets how bright that layer is. `brightness` does not widen
reach; reach is purely `layers[i].sigma_px`-driven.
The defaults are intentionally broad compatibility values; authored styles usually look better
with narrower sigmas like `2.0/4.0/8.0` for barriers, `2.0/4.0/10.0` for notes/flashes, and
`0.5/1.0/2.0` for small particles. Large sigmas add light across the entire scene and can wash out
the image instead of reading as a local glow.
**RON serializes a fixed-size `[GlowLayer; 3]` array with tuple parens `layers: (...)`, not
brackets `layers: [...]`**.

`edge_blend_px` (default `0.0`, notes only — see below) is how many pixels inward from the note's
edge the rim described above takes to fade back to the note's true fill color — independent of
`layers[0].sigma_px` (the corona's own innermost falloff distance), so you can tune how gradual the
handoff *looks* without changing how far the corona itself visually reaches. `0.0` falls back to
`layers[0].sigma_px`, matching the behavior before this field existed. Larger values (try
`4.0`–`10.0`) spread the blend over more pixels for a smoother, more gradual look; smaller values
snap to the corona's color more abruptly, closer to a hard seam. **Only wired up for
`NoteLayer::glow`** (`crates/render/src/notes/shader.wgsl`'s `fs_core`) — `BarrierLayer::glow`
parses and round-trips the field (it's the same shared `Glow` struct) but `barrier.wgsl` doesn't
read it yet, so it's currently a no-op there.

## Barrier layer (`BarrierLayer`)

The horizontal line/bar where falling notes stop.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `color` | `ColorBinding` | `Constant([255,255,255])` | Bar color. |
| `thickness` | `f32` | `4.0` | Bar thickness in **canvas pixels** (not on-screen/logical UI points — depends on preview scale, same coordinate space the falling notes render in). |
| `glow` | `Option<Glow>` | `None` | Soft radiating halo around the bar; presence is the on/off switch. |
| `pulse` | `Option<Pulse>` | `None` | Brief brighten-then-decay on each note arrival. |
| `wavy` | `Option<WavySpec>` | `None` | Rippling "calm ocean" edge instead of a flat line. Drives off deterministic transport time (`time_seconds` after subtracting sync offset) — so it's frame-reproducible in export and freezes exactly on pause/scrub, not wall-clock/frame-count based. |
| `show_bar` | `bool` | `false` | Whether the flat/opaque bar itself renders at all. Use `false` when `glow` is `Some(..)` so the barrier is pure glow; use `true` mainly as the fallback visible line when `glow` is `None`. |

```ron
barrier: Static((
    color: Constant((255, 220, 120)),
    thickness: 6.0,
    glow: Some((
        color: Constant((255, 220, 120)),
        brightness: 1.5,
        layers: (
            (amplitude: 3.0, sigma_px: 2.0),
            (amplitude: 2.0, sigma_px: 4.0),
            (amplitude: 0.85, sigma_px: 8.0),
        ),
    )),
    pulse: Some((decay_seconds: 0.35, brightness: 1.6)),
    wavy: Some((amplitude_px: 6.0, wavelength_px: 220.0, speed: 18.0, mode: Edge)),
    show_bar: false,
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
[Brightness/overexposure](#brightnessoverexposure). The default `1.6` is a tune-by-eye starting
point, not derived from anything; adjust per style if it looks too subtle or too blown-out.

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

`None` (the default) means a perfectly flat edge.

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
            (amplitude: 3.0, sigma_px: 0.5),
            (amplitude: 2.0, sigma_px: 1.0),
            (amplitude: 0.85, sigma_px: 2.0),
        ),
    )),
    flash: Some((
        radius_x_px: 40.0, radius_y_px: 40.0,
        color: Constant((255, 255, 255)), decay_seconds: 0.15, mode: Instant, brightness: 1.0,
        layers: (
            (amplitude: 2.6, sigma_px: 2.0),
            (amplitude: 1.1, sigma_px: 5.0),
            (amplitude: 0.38, sigma_px: 10.0),
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
| `layers` | `[GlowLayer; 3]` | Default tight/mid/wide, same as `Glow`. **Only read when `additive: true`** — non-additive "puff" particles ignore this field entirely and render as a plain hard-edged dot, unaffected by any value here. |

`EmissionMode::Continuous { rate_per_second }`: particles spawn every frame a note is held,
spread across the *width* of its key (not its center point) — reads as the key being "ground
down" rather than sparking once. `count` has no effect in this mode.

### `FlashSpec`

| Field | Type | Meaning |
|---|---|---|
| `radius_x_px` / `radius_y_px` | `f32` | Independent horizontal/vertical radii — set equal for a circular flash, unequal for an ellipse. |
| `color` | `ColorBinding` | Flash color. |
| `decay_seconds` | `f32` | How long the fade-out takes (see `mode` for when the fade *starts*). |
| `mode` | `FlashMode` | `Instant` (default) or `Sustained`. |
| `brightness` | `f32` | Default `1.0`. Baked into `layers[i].amplitude` at spawn time (a plain multiply, not a `hot_color` mix — see [Brightness/overexposure](#brightnessoverexposure)). `1.0` is a no-op. |
| `layers` | `[GlowLayer; 3]` | Default tight/mid/wide, same as `Glow`. A flash is always additive, so this is always read (unlike `ParticleSpec::layers`, which non-additive particles ignore). |

A flash always renders additively. It is fully "on" at spawn/at the start of its hold (see
`mode`), fading to 0 over `decay_seconds`.

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
`brightness: f32` knob (default `1.0`). Opaque surfaces use `hot_color`: at `brightness <= 1.0`,
the base color is dimmed by multiplication; above `1.0`, it desaturates toward white. Additive
coronas instead multiply the layered light contribution:

```text
light = color * sum(layer.amplitude * exp(-distance / layer.sigma_px)) * brightness
```

`brightness` does not change reach. Reach is controlled by `layers[i].sigma_px`; brightness only
changes intensity. Design history for earlier brightness/glow models lives in
`docs/fmstyle-history.md`.

Where it lives per effect:

- **Note glow / barrier glow** (`Glow::layers`/`Glow::brightness`): drives both the new
  `fs_glow` additive pass (halo) and the `hot_color` mix on the surface's own opaque fill
  (`notes/shader.wgsl`/`barrier.wgsl`).
- **Barrier pulse** (`Pulse::brightness`): peak brightness at `pulse = 1.0`, decaying back to the
  bar's resting brightness (`Glow::brightness`, or `1.0`); this value also feeds the corona's
  brightness multiplier during the pulse.
- **Flash / particles** (`FlashSpec::layers` / `ParticleSpec::layers`, both additive-only paths):
  bake `layers[i].amplitude * brightness` into a GPU instance before upload (a plain multiply, not
  a `hot_color` mix — there's no separate opaque core to whiten here), then run the identical
  additive layered-sum formula in `effects.wgsl`'s `fs_glow`. Non-additive "puff" particles are
  unaffected — `layers` is simply not read on that path.

## Known schema-only/no-op fields

One place to check when a parsed field does not affect rendering:

- **`NoteLayer::border` (`Border`)**: `struct Border { color: ColorBinding, width_px: f32 }` parses
  and round-trips but no renderer draws it.
- **Any non-`Constant` `ColorBinding`/`ScalarBinding`** (`ByVelocity`/`ByPitchClass`/`ByTrack`):
  parse and round-trip, resolve to a fixed fallback constant as described above, and are not yet
  driven by real per-note velocity/pitch/track data.

## Migration history

If a previously-working `.fmstyle.ron` file fails to load after an upgrade, check
`docs/fmstyle-history.md`. The schema-breaking changes so far are:

- `FlashSpec.radius_px` -> `radius_x_px` / `radius_y_px`
- `BarrierLayer.kind` + `glow_radius_px` -> `glow: Option<Glow>`
- `intensity` removed from `Glow`, `Pulse`, and `FlashSpec`
- `Glow.radius_px` -> `layers: [GlowLayer; 3]`
