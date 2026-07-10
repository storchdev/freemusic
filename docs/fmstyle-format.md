# `.fmstyle.ron` format reference

This is the field-by-field spec for the `.fmstyle.ron` visual style format. This document is the
living contract of what's actually in the schema today; design history and migration notes live in
`docs/fmstyle-history.md`. Keep both in sync when the schema changes.

Schema lives in `crates/project/src/style.rs`; keep that module's doc comment pointing back here.

## Overview

A `.fmstyle.ron` file is a [RON](https://github.com/ron-rs/ron) document describing a `Style`. It's
loaded via `Style::load(path)` and imported into a running project through the Project tab's
"Import style…" button (native file picker) or the path text field underneath it (type/paste a
path, then click "Load" or press Enter — `app/src/ui.rs::draw_project_tab`) — there's no other way
to attach one to a project today. An imported style fully overrides the legacy note/barrier "quick
control" sliders for whichever layers it sets; a project with no imported style has its look
synthesized from those sliders via `Style::from_legacy` (see each layer section below for the
exact legacy mapping).

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
enum Fill {
    Solid(ColorBinding),
    VerticalGradient { top: ColorBinding, bottom: ColorBinding },
    CanvasGradient { top: ColorBinding, bottom: ColorBinding },
}
```

`Solid` gives every note one color; `VerticalGradient`'s `top`/`bottom` are independently resolved
`ColorBinding`s (each darkened separately for sharp keys under `BlackKeyFill::Auto`), blended
across **each note's own on-screen height** — every note shows the full `top`->`bottom` range no
matter where it currently sits on the canvas.

`CanvasGradient` (Phase P) has the identical `{ top, bottom }` shape but blends across a fixed span
of **the canvas itself** instead: canvas Y = 0 (top of the frame) resolves to `top`, the barrier
line resolves to `bottom`, clamped beyond either end. Practically: whatever note is currently
passing through a given on-screen height shows the same color there, regardless of pitch/key — a
falling note's color shifts as it descends rather than each note carrying a fixed top-to-bottom
range baked in. `VerticalGradient` and `CanvasGradient` are mutually exclusive by construction
(`Fill` is one enum, a note's fill is exactly one variant); a note can still combine `CanvasGradient`
with `sheen`/`glow` exactly as it would with `Solid`/`VerticalGradient` — neither reads or affects
`Fill` at all, they're computed independently in the fragment shader. `black_key_fill` works
identically to the other variants: `Auto` darkens the resolved `top`/`bottom` endpoints for sharp
keys (still canvas-blended), `Same` uses the identical endpoints, and `Custom(fill)` can give sharp
keys a wholly independent fill — including a *different* variant than the natural-key fill (e.g.
natural keys `CanvasGradient`, sharp keys plain `Solid`) — see `NoteInstance::canvas_gradient`'s
doc comment in `crates/render/src/notes/instance.rs` for how the renderer keeps the two key groups'
bases independent.

Render-side: `crates/render/src/notes/mod.rs::rebuild_instances` resolves `Fill` to a `(color_top,
color_bottom)` pair exactly like `VerticalGradient` (`resolve_fill_base` handles both identically),
but also bakes a per-note `canvas_gradient: bool` flag (`is_canvas_gradient`) alongside it.
`shader.wgsl`'s `fill_color` reads that per-instance flag to pick which UV basis to mix
`color_top`/`color_bottom` across — the note-local fraction (`VerticalGradient`/`Solid`'s behavior)
or `in.position.y / (view_uniform.size.y * view_uniform.barrier_fraction)` (the canvas fraction,
using the fragment's absolute framebuffer position instead of its position within the note).

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
struct Glow {
    color: ColorBinding, brightness: f32, layers: [GlowLayer; 3], edge_blend_px: f32,
    match_note_color: bool,
}
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

`match_note_color` (default `false`) makes the corona/rim ignore `color` and instead tint itself
with the note's own fill color, sampled at whichever point on the note's silhouette the corona
fragment is closest to — so a note using `Fill::VerticalGradient`/`CanvasGradient` (and/or `Sheen`)
gets a halo that itself varies to match the fill directly beneath it, rather than one fixed halo
color for the whole note. **Only meaningful for `NoteLayer::glow`** — `BarrierLayer::glow` has no
per-note fill to sample, so this field is a documented no-op there (`barrier.wgsl` never reads it),
same precedent as `edge_blend_px` being notes-only. `false` is an exact no-op.

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
struct WavySpec {
    amplitude_px: f32, wavelength_px: f32, speed: f32, mode: WavyMode, slide_speed: f32,
    strands: Option<StrandSpec>,
}
```

- `amplitude_px`: peak vertical displacement in canvas pixels. The waveform is a sum of three
  incommensurate-frequency sine terms weighted 0.6/0.3/0.1 (sums to 1.0), so `|offset| <=
  amplitude_px` always holds exactly — a "calm ocean cross-section," not one obvious repeating
  sine.
- `wavelength_px`: pixels per cycle of the dominant (slowest) term.
- `speed`: how fast the ripple pattern *mutates in place* over transport time — which parts of the
  noise field currently look big/small, not the field's x-position; `0` freezes the shape (still
  x-varying, just not animating), but even a frozen shape sits at a fixed spot along x. See
  `slide_speed` below for actual lateral movement.
- `slide_speed` (default `0.0`): how fast the ripple pattern's noise field itself translates
  sideways along the barrier's width, in canvas px/second — independent of `speed`. A positive
  value gives a "current flowing through the wire" look: the whole ripple (and, since strands
  re-sample the same field, the whole strand bundle too — see `StrandSpec` below) visibly crawls
  sideways rather than just wobbling in place. `0.0` is an exact no-op.
- `mode` (`WavyMode`, default `TopWave`):
  - `TopWave`: only the top edge ripples, bottom stays flat — bar thickness varies across its
    width, can pinch thin at wave troughs.
  - `Edge`: the identical offset applies to both edges — the whole bar rigidly translates
    (constant thickness), reads as a thin curvy line rather than a bar with volume.
  - `FullWave`: both edges bulge outward together, correlated with the same wave, guaranteeing
    thickness is always `>= thickness` (never pinches below the configured value) while still
    swelling at wave crests.

`None` (the default) means a perfectly flat edge.

### `StrandSpec`

```rust
struct StrandSpec {
    count: u32, spread_px: f32, jitter: f32, thickness_px: f32,
    halo_amplitude: f32, halo_sigma_px: f32, glow_intensity: f32, flicker_speed: f32,
}
```

Independent thin filament threads fraying off the wavy top edge — the SeeMusic-style look where
the top edge doesn't read as one smooth wavy line but several fine threads scattered just above
it. Ported from `explorations/barrier-fx-lab/barrier-fx-lab.html`'s "Wavy edge" strand controls
(that lab's separate sliding-filament/wisp controls are a different, not-yet-ported experiment).

```ron
wavy: Some((
    amplitude_px: 7.0, wavelength_px: 55.0, speed: 3.5, mode: Edge,
    strands: Some((
        count: 6, spread_px: 16.0, jitter: 0.8, thickness_px: 1.3,
        halo_amplitude: 1.0, halo_sigma_px: 6.0, glow_intensity: 1.5, flicker_speed: 2.0,
    )),
)),
```

- `count` (default `5`): number of independent threads, capped at 8 (`barrier.wgsl`'s loop bound —
  extra strands beyond 8 are silently ignored).
- `spread_px` (default `14.0`): height (canvas px) of the furthest-out strand above the real top
  edge; strands in between are spaced evenly from `0` (riding the edge) to this value.
- `jitter` (default `0.75`, range `0..1`): `0.0` makes every strand ripple in lockstep with the
  main edge, just offset in height; `1.0` fully decorrelates each strand's ripple from the others
  and from the main edge.
- `thickness_px` (default `1.4`): thread thinness — smaller reads as a finer wire (`exp(-dist_px /
  thickness_px)` falloff around each strand's centerline).
- `halo_amplitude` / `halo_sigma_px` (defaults `1.0` / `6.0`): a soft additive halo around each
  thread, same falloff shape `GlowLayer` uses, but one shared amplitude/sigma pair applied
  identically to every strand rather than per-strand values.
- `glow_intensity` (default `1.3`): overall multiplier on the strand bundle's contribution to the
  corona.
- `flicker_speed` (default `1.8`): how fast each strand's brightness flickers over transport time.

Two restrictions worth calling out explicitly:

- **Strands are tinted by the barrier's own `Glow::color`/brightness — there is no separate strand
  color.** They render inside the corona (`fs_glow`) pass, so `BarrierLayer::glow` must be
  `Some(..)` for strands to be visible; `strands: Some(..)` with `glow: None` parses and
  round-trips but renders nothing, since no corona pass runs to draw them in.
- **Only meaningful when `mode` is `Edge`.** `TopWave`/`FullWave` describe the bar's own solid
  cross-section (its thickness rippling/swelling), not a bundle of independent threads riding
  alongside a rigidly-translating edge — `barrier.wgsl`'s `fs_glow` checks the live `mode` uniform
  and skips the whole strand loop outside `Edge`, even if `strands` is `Some(..)`. This is the
  single source of truth: there is no matching CPU-side gate, `BarrierRenderer::set_style` uploads
  strand params unconditionally whenever `strands` is `Some(..)`.

Strands have no `slide_speed` of their own — they re-sample the same `wavy_offset_seeded` noise
field `WavySpec::slide_speed` already translates, so setting a nonzero `slide_speed` on the parent
`WavySpec` moves the whole strand bundle sideways along with the base edge, in lockstep.

See `examples/styles/barrier-strands.fmstyle.ron` for a complete, renderable example.

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
| `color` | `ParticleColor` | How each particle's color is chosen — see below. |
| `additive` | `bool` | Additive blending (bright, overlapping particles glow) vs. premultiplied-alpha (opaque-ish). Decided once per `update()` call from the layer's currently-resolved value — a particle spawned under one style doesn't retroactively update if a *different* style is imported while it's still alive. |
| `emission` | `EmissionMode` | `Burst` (default) or `Continuous { rate_per_second }`. |
| `brightness` | `f32` | Default `1.0`. Baked into `layers[i].amplitude` at spawn time (a plain multiply, not a `hot_color` mix — see [Brightness/overexposure](#brightnessoverexposure)). `1.0` is a no-op. |
| `layers` | `[GlowLayer; 3]` | Default tight/mid/wide, same as `Glow`. **Only read when `additive: true`** — non-additive "puff" particles ignore this field entirely and render as a plain hard-edged dot, unaffected by any value here. |

`EmissionMode::Continuous { rate_per_second }`: particles spawn every frame a note is held,
spread across the *width* of its key (not its center point) — reads as the key being "ground
down" rather than sparking once. `count` has no effect in this mode.

### `ParticleColor`

```rust
enum ParticleColor {
    Fixed(ColorBinding),
    MatchNote,
    YGradient {
        top: ColorBinding,
        bottom: ColorBinding,
        top_fraction: f32,    // default 0.0
        bottom_fraction: f32, // default 0.8
    },
}
```

A single mutually-exclusive mode selector (not independent toggles) — a particle's color comes
from exactly one source:

- `Fixed(binding)` (default: white): every particle from this spec gets the same resolved color —
  the only behavior that existed before this enum did.
- `MatchNote`: every particle spawned for a given note is colored by *that note's own* fill at
  whichever point is currently crossing the barrier — see
  [Note color sampling at the barrier](#note-color-sampling-at-the-barrier) below for exactly what
  this does and doesn't reflect. One color per note (not per particle), resolved once per note per
  frame and reused for every particle it spawns that frame — under `EmissionMode::Continuous` this
  is what makes a held note's particle stream slide across the note's own gradient over time
  instead of staying pinned to its arrival color.
- `YGradient { top, bottom, top_fraction, bottom_fraction }`: particles are tinted by their own
  *current* canvas Y position, blended between `top` (at `top_fraction`) and `bottom` (at
  `bottom_fraction`) — each a fraction of canvas height (`0.0` = top of frame, `1.0` = bottom),
  clamped rather than extrapolated outside that span. Unlike `Fixed`/`MatchNote` (baked once at
  spawn), this is recomputed every frame as a particle falls/rises under `gravity_px`, so a
  particle visibly shifts color as it moves. `top_fraction`/`bottom_fraction` default to `0.0`/
  `0.8` (this field's original hardcoded span: top of frame to a typical default barrier
  position), but since particles spawn at the barrier and rarely travel far from it, most of a
  particle's actual on-screen motion tends to land in a narrow sliver near the `bottom_fraction`
  end of that default span — the color barely changes across it. Narrow the span to bracket where
  particles in your own spec actually travel (e.g. a bit above and below the barrier) to make the
  gradient visible.

### `FlashSpec`

| Field | Type | Meaning |
|---|---|---|
| `radius_x_px` / `radius_y_px` | `f32` | Independent horizontal/vertical radii — set equal for a circular flash, unequal for an ellipse. |
| `color` | `FlashColor` | How the flash's color varies across its own width — see below. |
| `decay_seconds` | `f32` | How long the fade-out takes (see `mode` for when the fade *starts*). |
| `mode` | `FlashMode` | `Instant` (default) or `Sustained`. |
| `brightness` | `f32` | Default `1.0`. Baked into `layers[i].amplitude` at spawn time (a plain multiply, not a `hot_color` mix — see [Brightness/overexposure](#brightnessoverexposure)). `1.0` is a no-op. |
| `layers` | `[GlowLayer; 3]` | Default tight/mid/wide, same as `Glow`. A flash is always additive, so this is always read (unlike `ParticleSpec::layers`, which non-additive particles ignore). |

A flash always renders additively. It is fully "on" at spawn/at the start of its hold (see
`mode`), fading to 0 over `decay_seconds`.

### `FlashColor`

```rust
enum FlashColor {
    Solid(ColorBinding),
    HorizontalGradient(Vec<ColorBinding>),
    MatchNote,
}
```

- `Solid(binding)` (default: white): one flat color, resolved once — the only behavior that
  existed before this enum did.
- `HorizontalGradient(stops)`: a hand-authored horizontal gradient — any number of evenly-spaced
  left-to-right `ColorBinding` stops across the flash's own width (`2 * radius_x_px`), including
  just one (equivalent to `Solid`). The renderer resamples this list onto its fixed internal
  5-stop representation at spawn time, so any stop count works.
- `MatchNote`: auto-derived from the note that triggered this flash — one flat color, sampled from
  whichever point of that note is currently at the barrier, the same mechanism
  `ParticleColor::MatchNote` uses (see
  [Note color sampling at the barrier](#note-color-sampling-at-the-barrier)). Internally this fills
  the flash's gradient stops with that one color (the same "uniform stops" trick a plain particle
  color already uses), so it renders identically to `Solid` with that color at any given instant —
  the distinction is purely *which* color and that it keeps re-evaluating over time, not that this
  variant makes the flash itself multicolored at once. Under `FlashMode::Sustained` this
  re-evaluates every frame for as long as the flash stays lit, so a held note's glow slides across
  the note's own gradient rather than staying pinned to its arrival color. For a genuinely
  multicolor flash, use `HorizontalGradient` instead (optionally hand-picking colors that
  complement the note, e.g. reading them off the style's own `NoteLayer::fill`).

### Note color sampling at the barrier

`ParticleColor::MatchNote` and `FlashColor::MatchNote` both resolve to **the triggering note's own
fill color at whichever point is currently crossing the barrier**
(`render::notes::NoteInterval::color_at_barrier`) — one flat color per note, evaluated fresh every
frame rather than a style-wide constant or a value fixed at the note's arrival, but not a finer
per-pixel sample of that note's actual rendered fill.

Concretely, `color_at_barrier` mixes the note's own resolved `color_bottom` (its leading edge —
the color visible right at arrival) toward `color_top` (its trailing edge) by how far the note has
been held past arrival, mirroring `shader.wgsl`'s note-local fill math evaluated at the barrier's
fixed canvas position. This is why the two `MatchNote` variants no longer talk about "the note's
bottom color" the way an earlier version of this feature did: for a `Solid` or flat-colored note
the leading- and trailing-edge colors are identical, so sampling only ever "the bottom" and
sampling "whichever part is at the barrier" looked the same — but for a `VerticalGradient` note
(or a flash/particle stream spawned continuously while a note is held), the part of the note
actually crossing the barrier keeps advancing from the leading edge toward the trailing edge, so a
fixed "always the bottom" sample would visibly stop matching the note's own color partway through
a long-held note. `Fill::CanvasGradient` notes are the one case where this collapses back to a
constant: that gradient is keyed to canvas position, not note-local progress, and the barrier is a
fixed canvas position, so the color there is always the gradient's own barrier-line endpoint
regardless of how long the note has been held.

For a burst/`FlashMode::Instant` trigger this is evaluated once, right as the note arrives, so it
behaves exactly as a "match the color at arrival" sample would — the general barrier-tracking
formula is a strict superset of that behavior, not a separate mode. It only visibly differs from a
fixed arrival-color sample for `EmissionMode::Continuous` particles and `FlashMode::Sustained`
flashes, both of which stay active for a note's whole held duration.

An earlier version of this feature instead sampled several points *across* a note's leading edge,
specifically to reproduce a diagonal `Sheen` stripe's horizontal brightening band (the only thing
that varies a note's color left-to-right — plain `Fill::Solid`/`VerticalGradient`/`CanvasGradient`
are all uniform across a note's width, only ever varying color top-to-bottom or by canvas height).
That was retracted: it meant hand-porting `shader.wgsl`'s fill/sheen math into Rust
(`render::notes::mod.rs`, since removed), which only stayed correct for sheen specifically — any
*other* future note-color effect (a different stripe/pattern, a texture, anything not already
mirrored in that Rust port) would silently be invisible to `MatchNote` while still rendering
correctly on the note itself, a maintenance trap that would only get worse as note styles grow.
The `(color_top, color_bottom)` pair alone doesn't have this problem: every `Fill` variant, current
or future, resolves to that pair by construction (`resolve_fill_base`'s contract), so `MatchNote`
stays correct with zero additional code for anything built on top of that contract — the tradeoff
is giving up the sheen-driven cross-section fidelity in exchange for that guarantee.

Note that `Glow::match_note_color` (the *note's own* halo/rim matching its own fill — a different
mechanism from `ParticleColor`/`FlashColor::MatchNote`) doesn't have this tradeoff at all: it calls
back into the note shader's own `fill_color_at` function rather than a separate Rust port, so it
automatically reflects sheen (and any future fill-affecting change) with no porting required.

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

Every color-typed field in this schema is a `ColorBinding`, meant to vary per note by velocity,
pitch class, or track. **Wherever an actual note is in scope — note fill, particle/flash colors
triggered by a note — all four variants resolve per note** via `ColorBinding::resolve_for_note
(velocity, pitch, track_id)`:

- `Constant(color)` → `color`, ignoring the note entirely (as before).
- `ByVelocity(ramp)` → linearly interpolates `ramp.low` (velocity 0) to `ramp.high` (velocity
  127) by this note's own MIDI velocity.
- `ByPitchClass(colors)` → `colors[pitch % 12]` (0 = C, 1 = C#, ... 11 = B, independent of
  octave), by this note's own MIDI pitch number.
- `ByTrack(colors)` → `colors[track_id % colors.len()]` (wrapping, so a style authored for fewer
  colors than a MIDI file has tracks still resolves deterministically instead of panicking), or
  white if `colors` is empty, by this note's own track index.

A handful of fields have no single note to resolve against — the canvas `background`, the barrier
bar/glow (`BarrierLayer::color`/`BarrierLayer::glow.color`, one bar for the whole canvas), and the
note-glow GPU uniform (`NoteLayer::glow.color`, one value shared by every note's shader
invocation unless `Glow::match_note_color` is set, which samples the note's own per-note-resolved
fill color instead). These call `ColorBinding::resolve_constant()` instead, a **permanent, not a
"not yet wired up"**, fallback to a single representative value:

- `ByVelocity(ramp)` → `ramp.high` (the loudest-note color).
- `ByPitchClass(colors)` → `colors[0]`.
- `ByTrack(colors)` → `colors.first()`, or white if the list is empty.

`ScalarBinding` (currently unused by any field in this schema, reserved for a future numeric
per-note property) only has `resolve_constant()` so far, with the identical fallback rule
(`ByVelocity.high`, `ByPitchClass[0]`, `ByTrack.first()` or `1.0`) — it would need its own
`resolve_for_note` once some field actually reads it.

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
- **Any non-`Constant` `ColorBinding` used on `background`/`BarrierLayer::color`/
  `BarrierLayer::glow.color`/`NoteLayer::glow.color`**: parses and round-trips, but resolves to a
  fixed representative constant (`resolve_constant()`) rather than varying per note — see the
  `ColorBinding`/`ScalarBinding` section above for why these specific fields structurally have no
  single note to resolve against. Every other `ColorBinding` field (note fill, particle/flash
  colors) *does* vary per note (`resolve_for_note()`).
- **`ScalarBinding`**: schema/parsing only — no field in this schema uses it yet.

## Migration history

If a previously-working `.fmstyle.ron` file fails to load after an upgrade, check
`docs/fmstyle-history.md`. The schema-breaking changes so far are:

- `FlashSpec.radius_px` -> `radius_x_px` / `radius_y_px`
- `BarrierLayer.kind` + `glow_radius_px` -> `glow: Option<Glow>`
- `intensity` removed from `Glow`, `Pulse`, and `FlashSpec`
- `Glow.radius_px` -> `layers: [GlowLayer; 3]`
- `ParticleSpec.color: ColorBinding` -> `color: ParticleColor` (wrap an existing `Constant(...)`
  etc. value as `Fixed(...)`)
- `FlashSpec.color: ColorBinding` -> `color: FlashColor` (wrap an existing `Constant(...)` etc.
  value as `Solid(...)`)
- `ParticleColor::MatchNoteBottom` -> `MatchNote`, `FlashColor::MatchNoteBottom` -> `MatchNote`
  (rename only, no other field shape change — see
  [Note color sampling at the barrier](#note-color-sampling-at-the-barrier) for why "bottom" no
  longer describes what these sample)
