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
)
```

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
    glow: Some((color: Constant((120, 200, 255)), radius_px: 12.0, intensity: 0.5)),
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
struct Glow { color: ColorBinding, radius_px: f32, intensity: f32 }
```

Halo color/radius/strength. The rendered quad is inflated by `radius_px` on every side so there are
pixels to paint the halo onto (an exact no-op when `glow` is `None` — the inflation margin is
`0.0`).

## Barrier layer (`BarrierLayer`)

The horizontal line/bar where falling notes stop.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `kind` | `BarrierKind` | `Line` | `Line` = flat bar, no glow; `Glow` = same bar with a radiating halo. |
| `color` | `ColorBinding` | `Constant([255,255,255])` | Bar color. |
| `thickness` | `f32` | `4.0` | Bar thickness in **canvas pixels** (not on-screen/logical UI points — depends on preview scale, same coordinate space the falling notes render in). |
| `glow_radius_px` | `f32` | `0.0` | Halo radius; only visible under `kind: Glow`. |
| `pulse` | `Option<Pulse>` | `None` | Brief brighten-then-decay on each note arrival. |
| `wavy` | `Option<WavySpec>` | `None` | Rippling "calm ocean" edge instead of a flat line. Drives off deterministic transport time (`time_seconds` after subtracting sync offset) — so it's frame-reproducible in export and freezes exactly on pause/scrub, not wall-clock/frame-count based. |

```ron
barrier: Static((
    kind: Glow,
    color: Constant((255, 220, 120)),
    thickness: 6.0,
    glow_radius_px: 24.0,
    pulse: Some((intensity: 0.8, decay_seconds: 0.35)),
    wavy: Some((amplitude_px: 6.0, wavelength_px: 220.0, speed: 18.0, mode: Edge)),
)),
```

### `Pulse`

```rust
struct Pulse { intensity: f32, decay_seconds: f32 }
```

Stateless: recomputed every frame from the sorted note-onset list (no spawn/tracking bookkeeping),
so it's correct under scrubbing in either direction with no special-casing.

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
        additive: true, emission: Burst,
    )),
    flash: Some((
        radius_x_px: 40.0, radius_y_px: 40.0, intensity: 0.9,
        color: Constant((255, 255, 255)), decay_seconds: 0.15, mode: Instant,
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

`EmissionMode::Continuous { rate_per_second }`: particles spawn every frame a note is held,
spread across the *width* of its key (not its center point) — reads as the key being "ground
down" rather than sparking once. `count` has no effect in this mode.

### `FlashSpec`

| Field | Type | Meaning |
|---|---|---|
| `radius_x_px` / `radius_y_px` | `f32` | Independent horizontal/vertical radii — set equal for a circular flash, unequal for an ellipse. **Renamed from a single `radius_px` in Phase H** — see the changelog. |
| `intensity` | `f32` | Peak brightness. |
| `color` | `ColorBinding` | Flash color. |
| `decay_seconds` | `f32` | How long the fade-out takes (see `mode` for when the fade *starts*). |
| `mode` | `FlashMode` | `Instant` (default) or `Sustained`. |

`FlashMode`:
- `Instant`: decays over `decay_seconds` starting immediately at note-on — a quick pulse.
- `Sustained`: holds at full `intensity` for the note's entire held duration, only starting to
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

If a previously-working `.fmstyle.ron` file fails to load after an upgrade, check this table first
— Phase H's rename is the only schema-breaking change so far.
