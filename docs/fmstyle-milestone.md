# The `.fmstyle.ron` extensible visual style milestone

Full phase-by-phase narrative (Phases A–L) of the extensible style format work, split out of
CLAUDE.md. For the field-by-field format spec see `docs/fmstyle-format.md`.

### Extensible visual style format (Phase A of the `.fmstyle.ron` milestone)

Full plan: `~/.claude/plans/potentially-very-big-milestone-vectorized-seal.md`. This milestone's
goal is a data-driven `.fmstyle.ron` format that can describe note fills (gradient/sheen/glow),
barrier looks (glow/pulse), and barrier-hit transitions (particles/flash) — proving visuals can be
authored as data, not just via the existing color/roundedness/thickness sliders. **Phase A (format
+ plumbing, no visual change) is done; Phases B-F (vendoring the note renderer, actually drawing
any of this, sample-style screenshots) are not started.**

**For the field-by-field `.fmstyle.ron` contract (defaults, meaning, RON snippets, breaking-change
log), see `docs/fmstyle-format.md`** — the phase write-ups throughout this section (here and
below) stay as historical narrative of what was built and why, not rewritten to match; the doc is
the living spec, kept in sync whenever the schema changes.

- **New module `crates/project/src/style.rs`** (re-exported flat from `crates/project/src/lib.rs`,
  same pattern as the other `project` types): `Style { version, notes: Timed<NoteLayer>, barrier:
  Timed<BarrierLayer>, transition: Timed<TransitionLayer> }`, with `Style::load`/`save` mirroring
  `Project::load`/`save` exactly (`Result<_, String>`, same RON pretty-printer). Every field is
  `#[serde(default)]`-compatible so older/partial files still load — verified by a unit test that
  strips the whole `style` line out of a serialized `Project` and confirms it still parses with
  `style: None`.
  - `Timed<T> = enum { Static(T), Keyed(Vec<(f64, T)>) }` is the time-keying spine:
    `resolve(t)` returns the last key `<= t`, clamped to the first key if `t` precedes all of
    them. **v1 never actually re-resolves during playback** — nothing calls `resolve` at any time
    other than a one-time `resolve(0.0)` once real rendering consumes a `Style` (Phase B+); the
    type and its boundary behavior are tested now so the spine is provably correct before
    anything is built on top of it.
  - `ColorBinding`/`ScalarBinding` are the per-note property-binding spine: `Constant` resolves
    exactly; `ByVelocity`/`ByPitchClass`/`ByTrack` parse and round-trip but aren't wired to real
    per-note data yet — `resolve_constant()` falls back to a representative constant (ramp's high
    end / first pitch-class entry / first track entry, warned once via a `std::sync::Once` guard
    so a style using these doesn't spam stderr once Phase B+ actually calls this per-note). This
    is intentionally the smallest possible extension point: the enum shape exists so a future
    session can wire real velocity/pitch/track data through without a format break, but nothing
    downstream depends on that data existing yet.
  - `NoteLayer`/`BarrierLayer`/`TransitionLayer` (plus `Fill`, `Sheen`, `Glow`, `Border`,
    `BarrierKind`, `Pulse`, `TransitionKind`, `ParticleSpec`, `FlashSpec`) are the effect-layer
    schema exactly as scoped in the plan. `Border` is schema-only (parses, round-trips, nothing
    reads it) — a deliberately documented no-op, not a bug.
  - `Style::from_legacy(&NoteStyle, &BarrierStyle) -> Style` produces the exact look the existing
    sliders already draw (`Fill::Solid`, no sheen/glow, `BarrierKind::Line`,
    `TransitionKind::None`) — the intended single rendering path once Phase B lands: the renderer
    always consumes a `Style`, either imported or synthesized from the legacy fields. **Nothing
    calls `from_legacy` yet outside tests** — Phase A stops at having it exist and be correct;
    wiring it into `AppState::redraw` has no purpose until Phase B's renderer actually accepts a
    `Style` argument, so that wiring was deliberately left out rather than adding a call site with
    no consumer (would just be dead-looking plumbing).
- **`Project` gained `pub style: Option<Style>`** (`#[serde(default)]`), alongside the existing
  `barrier_style`/`note_style` "quick controls." `snapshot_project`/`load_project_from_path`/
  `new_project` in `app/src/main.rs` all thread it through, same one-line-per-field pattern every
  other `Project` field already uses.
- **"Import style…" button** (Project tab, `app/src/ui.rs::draw_project_tab`) is the only UI
  surface: `UiState.import_style_requested` follows the exact `open_project_requested` template
  (flag set by the button, consumed same-redraw in `main.rs`'s `apply_post_ui_updates` via an
  `rfd::FileDialog` `.ron`-filtered picker, `Style::load`, set into `ui_state.style`). A one-line
  label under the button ("Custom style imported…" / "Using note/barrier sliders…") is the only
  feedback — not in the original plan's UI list verbatim, but cheap and necessary so importing a
  style (which currently changes nothing visible, since Phase B hasn't landed) doesn't look like
  the button silently did nothing.
- **Sample styles shipped early, ahead of the plan's Phase F**: `examples/styles/{gradient-glow,
  barrier-pulse,sparks}.fmstyle.ron` (repo root, not under `crates/`) exercise gradient+sheen+glow,
  barrier glow+pulse, and particles+flash respectively. Generated via a throwaway example binary
  (`crates/project/examples/dump_sample_styles.rs`, run once with `cargo run -p project --example
  dump_sample_styles` and its stdout copied into the three files) rather than hand-typed RON —
  guarantees the checked-in files exactly match what this `ron` version actually serializes
  (notably: unit enum variant `None` serializes as the raw identifier `r#None`, since `None` is a
  reserved word; easy to get wrong by hand). A unit test (`shipped_sample_styles_parse`) reads
  every `.ron` file in that directory and calls `Style::load` on it, so the checked-in samples
  can't silently drift out of sync with the schema as it evolves. They're already importable via
  the button and parse correctly, but **importing one still changes nothing on screen** — the
  note pipeline (see Phase B below) doesn't read `project::style::Style` yet, only the
  `NoteStyle`/`BarrierStyle` "quick controls"; that wiring is Phase C+.
- **Verified so far**: `cargo build`/`scripts/check.sh` (fmt+clippy) clean; `cargo test --workspace`
  passes, including 8 new tests in `crates/project` (`Timed::resolve` boundaries — static, keyed
  mid-range, keyed before-first-key clamp, keyed past-last-key — `ColorBinding::Constant`/
  non-`Constant`-fallback resolution, `Style`/`Project` RON round-trips, old-`Project`-without-
  `style` loading, and the shipped-sample-styles parse check). **Not yet manually exercised in the
  running app** — worth clicking "Import style…", picking one of the three samples, and
  confirming the label under the button flips to "Custom style imported…" and a project save/load
  round-trips `style` correctly (nothing else to check visually until Phase C).
- **Phase C (note fill effects actually rendered) is now done** — see "Note fill effects: gradient,
  sheen, glow" further below. **Phase D (barrier promoted from egui overlay to a real glow/pulse
  render pass) is now done** — see "Barrier glow/pulse pass" further below. **Phase E (barrier-hit
  particle/flash transition pass) is now done** — see "Transition particles + flash pass" further
  below. **Not started**: the doc/screenshot cleanup this section originally called "Phase F" —
  superseded by the lettered-phase continuation below, which reuses letters F-J for a distinct set
  of features (see disambiguation note).

### Style extensibility continuation: Phases F-J (separate plan, new letter scheme)

Full plan: `~/.claude/plans/the-most-recent-changes-delightful-rabbit.md`. **Disambiguation**: this
plan reuses letters F-J for a *different* set of features than the "Phase F" mentioned just above
(which only ever meant "ship sample-style screenshots/docs" and was never built out under that
name) — don't confuse the two. This continuation closes four concrete gaps found while testing
Phases A-E, plus adds a real field-by-field spec doc: **F** separate white/black key colors, **G**
wavy "calm ocean" barrier edge, **H** elliptical/radiating flash (breaking rename), **I** continuous
"grinding" particles + sustained flash-as-glow, **J** `docs/fmstyle-format.md`. Order matters: H
before I because I's spawn code touches the same `EffectInstance`/spawn helpers H's rename touches.

**Phase F — separate white/black key colors — DONE.**
- **Schema** (`crates/project/src/style.rs`): `NoteLayer` gained `#[serde(default)]
  pub black_key_fill: BlackKeyFill`, a new enum `{ Auto (default, today's darken-by-0.6
  behavior), Same (no darkening), Custom(Fill) (independently resolved solid/gradient fill) }`.
- **Legacy quick control** (`crates/project/src/lib.rs`): `NoteStyle` gained
  `#[serde(default)] pub black_key_color: BlackKeyColorMode` (`Auto`/`Same`/`Custom([u8; 3])` —
  solid-only, mirroring `BlackKeyFill` minus gradient support). `Style::from_legacy` maps
  `Auto→Auto`, `Same→Same`, `Custom(c)→Custom(Fill::Solid(ColorBinding::Constant(c)))`.
- **Renderer** (`crates/render/src/notes/mod.rs`): extracted `resolve_fill_base(&Fill) -> ([u8;
  3], [u8; 3])` out of the inline match `rebuild_instances` used to do, shared by both the
  white-key fill resolution and (new) `BlackKeyFill::Custom`'s independent fill resolution.
  `BlackKeyFill::Auto`'s code path calls the exact same `darken(_, 0.6)` on the exact same base
  colors as before — verified byte-identical, the required no-regression guarantee for projects
  with no imported style and no touched black-key UI. Per-note sharp/white key color selection
  (`if key.kind().is_sharp() { dark } else { light }`) is unchanged.
- **UI** (`app/src/ui.rs::draw_keyboard_tab`): a `Auto`/`Same`/`Custom` `egui::ComboBox` next to
  the existing note "Color:" row, plus a second `color_edit_button_srgb` shown only when
  `Custom` is selected. Picking `Custom` for the first time seeds it with `darken(color, 0.6)`
  (new `ui.rs::darken_color` helper, matching the renderer's own darkening) rather than jumping
  to an arbitrary color. "Reset note style" already resets the whole `NoteStyle`, so it resets
  this new field too with no extra code.
- **Tests**: `crates/project/src/style.rs` gained `black_key_fill_custom_gradient_round_trips`,
  round-tripping a `BlackKeyFill::Custom(Fill::VerticalGradient{..})` through RON.
- **Sample styles**: `crates/project/examples/dump_sample_styles.rs`'s `NoteLayer` literal needed
  an explicit `black_key_fill: project::BlackKeyFill::Auto` (required even though the field is
  `#[serde(default)]`, since Rust struct literals don't get that leniency — only deserialization
  does). The three checked-in `examples/styles/*.fmstyle.ron` files were **not** regenerated
  wholesale (the generator's current output has since drifted slightly from the checked-in files
  on unrelated fields — e.g. `gradient-glow`'s sheen intensity/width/angle — so overwriting would
  have picked up unrelated changes); instead `black_key_fill: Auto,` was inserted directly into
  each file right after its existing `border: None,` line via a targeted `sed`, keeping every
  other value untouched. Confirmed by `shipped_sample_styles_parse` (still passes) that this
  hand-edit didn't break parsing.
- **Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), `cargo test --workspace` all clean.
  **Not yet manually run** (per the "never run the app yourself" rule) — worth confirming, next
  time someone has hands on the app: with no imported style, black keys still look exactly as
  before (Auto); switching the new combo box to `Same` makes black-key notes match white-key
  notes exactly; `Custom` + picking a distinct color changes only black-key notes; and importing a
  style with an explicit `black_key_fill: Custom(...)` overrides the Keyboard tab's quick control,
  same as every other imported field already does.

**Phase G — Barrier wavy "calm ocean" edge — DONE.**
- **Schema** (`crates/project/src/style.rs`): new `WavySpec { amplitude_px, wavelength_px, speed }`
  (re-exported from `crates/project/src/lib.rs` alongside the other `style` types).
  `BarrierLayer` gained `#[serde(default)] pub wavy: Option<WavySpec>` (`None` = flat edge, the
  only look before this phase). `Style::from_legacy`'s `BarrierLayer` literal sets `wavy: None`,
  keeping the legacy-slider look unchanged.
- **Uniform layout** (`crates/render/src/barrier.rs`): `Uniforms` gained a 4th all-vec4 field,
  `wave: [f32; 4]` (`x`=amplitude_px, `y`=wavelength_px, `z`=speed, `w`=transport time seconds),
  keeping the same "every field already vec4-aligned, no std140 column-padding to get wrong"
  convention `StyleUniform`/this struct's existing fields already followed. `flags.z` is the new
  wavy-enabled flag (`flags.x`/`flags.y` unchanged: glow-enabled/pulse-intensity). `set_style`
  writes `flags[2]`/`wave[0..2]` from `barrier_layer.wavy` (`None` zeroes all three, an exact
  disable). `update_pulse` gained one line, `self.data.wave[3] = time_seconds` — reuses the exact
  same sync-offset-subtracted, deterministic/export-reproducible clock every other animated
  element (note fall, barrier pulse) already reads, so the ripple freezes exactly on pause/scrub
  and is frame-reproducible in export. **No public method signature changed** — both
  `Compositor::update_barrier` call sites (`app/src/main.rs`, `crates/export/src/lib.rs`) needed
  no edits.
- **Shader** (`crates/render/src/barrier.wgsl`): `wavy_offset(x)` sums three
  incommensurate-frequency sine terms weighted 0.6/0.3/0.1 (sum to 1.0, so `|offset| <=
  amplitude_px` always holds exactly — not one literal sine, per the user's explicit "calm ocean
  cross-section, stochastic-looking but calm" ask) and returns `0.0` outright when `flags.z < 0.5`
  (wavy disabled) — an exact, not just visual, no-op. The vertex shader's rasterized-quad inflation
  (`half_extent`) gained a third additive margin, `wavy_margin = select(0.0, wave.x, flags.z >
  0.5)`, alongside the existing glow margin — extends the quad symmetrically top/bottom (simpler
  than an asymmetric top-only margin; the harmless extra overdraw below the always-flat bottom edge
  was an explicit tradeoff in the plan) so there are pixels to paint the rippling top edge onto.
- **By default only the top edge waves, the bottom stays flat** (a rippling surface over a flat
  floor, not a wobbling slab): the fragment shader computes `top_edge = barrier_y - half_thickness +
  wavy_offset(in.position.x)` and `bottom_edge = barrier_y + half_thickness` (unchanged), and
  `core_alpha` is now `alpha_top * alpha_bottom` (independent smoothsteps around each edge) instead
  of the old single symmetric `1 - smoothstep(half_thickness±1, |y - barrier_y|)`. **Re-derived by
  hand, not assumed** (per this codebase's standing rule for shader math changes, after the
  rotation-shear and barrier-fade-out bugs both slipped past `cargo build`/`clippy`): substituting
  `u = y - barrier_y`, the old formula for `u >= 0` is exactly `smoothstep(top_edge-1, top_edge+1,
  y)` after the corresponding shift, and for `u < 0` the mirror image around `bottom_edge` — so the
  product form reproduces the old formula exactly whenever the two smoothstep transition zones
  don't overlap (true for `thickness > 2px`), with `wave == 0` making `top_edge`/`bottom_edge`
  collapse back to the plain symmetric case.
- **Follow-up #1, same session**: after seeing this rendered, the user asked for an option to make
  the *whole bar* ride the wave (both edges moving together) instead of only the top surface
  bulging while the bottom stayed rigidly flat. First cut: `WavySpec::both_edges: bool`, wired as
  a 4th `flags` slot written by `set_style`, with `bottom_wave = select(0.0, wave, flags.z > 0.5
  && flags.w > 0.5)` added onto `bottom_edge` in the shader — both edges get the *identical*
  offset, so the bar translates rigidly (constant thickness).
- **Follow-up #2, same session**: after trying it, the user liked the rigid-translation look but
  said it wasn't what they'd actually asked for ("it just looks like a curvy line ... with even
  less volume") — the translation reads as a thin moving line precisely because the
  cross-section never changes shape — and asked for a genuine "double sided wave" as a **third**
  option alongside the original top-only look and the rigid-translation look they'd grown to
  like. Replaced the bool with a 3-state enum, since two independent booleans-worth of behavior
  needed to coexist. First cut of the third variant made the bottom edge ripple *out of phase*
  with the top (a `wavy_offset_bottom` with phase-shifted sine terms) — this was superseded a
  turn later, see Follow-up #3.
- **Follow-up #3, same session**: after trying the out-of-phase variant, the user said it
  pinched down to (near) zero thickness at some x positions and asked for the third mode to
  *always* have volume, swelling outward on both sides rather than ever cancelling down thin —
  and asked for all three variants to be renamed: `TopOnly`→`TopWave`, `Mirrored`→`Edge`,
  `BothEdges`→`FullWave`. Final shape:
  ```rust
  pub enum WavyMode { #[default] TopWave, Edge, FullWave }
  ```
  (`crates/project/src/style.rs`, re-exported from `crates/project/src/lib.rs` like every other
  `style` type). `TopWave` is the original default look (only the top ripples, can pinch thin —
  unchanged, this was never the complaint). `Edge` is the rigid-translation look the user liked.
  `FullWave` replaces the out-of-phase pairing: **both edges bulge outward together, correlated
  with the same underlying wave rather than an independent/decorrelated one** —
  `swell = 0.5 * (amplitude_px + wave)`, which is always in `[0, amplitude_px]` since `wave`
  itself is bounded to `[-amplitude_px, amplitude_px]`, then `top_offset = -swell` and
  `bottom_offset = +swell`. This guarantees thickness is always `base_thickness + 2*swell >=
  base_thickness` — never pinching below the configured thickness — while both edges still bulge
  most at the same x where the underlying wave peaks (rather than independently, which is what
  let the two edges cancel each other down to near-zero gap before this fix).
  - **Renderer** (`crates/render/src/barrier.rs`): `flags.w` holds the mode as a float (0/1/2),
    unchanged shape from Follow-up #2, just remapped to the new variant order/names.
  - **Shader** (`crates/render/src/barrier.wgsl`): the separate `wavy_offset_bottom` function was
    deleted — `FullWave` no longer needs its own trig pass, it derives `swell` directly from the
    same `wave` value `TopWave`/`Edge` already compute. `fs_main` now computes `top_offset`/
    `bottom_offset` per mode (`flags.w > 1.5` → `FullWave`'s swell pair; `> 0.5` → `Edge`'s
    identical `wave` on both; else `TopWave`'s `wave` on top only, `0.0` on bottom) — `TopWave`'s
    code path is byte-for-byte what it was before any of these follow-ups existed.
  - **No vertex-margin or glow-formula changes needed for any of these follow-ups**: the
    inflation margin was already symmetric top/bottom by `amplitude_px` regardless of mode (every
    mode's per-edge offset stays within `[-amplitude_px, amplitude_px]`, including `FullWave`'s
    `swell`), and the glow math already read `top_edge`/`bottom_edge` as variables (not hardcoded
    flat expressions), so both automatically extend correctly with no further changes.
  - **Samples**: `examples/styles/barrier-wavy.fmstyle.ron` (which the user had been
    hand-editing/tuning throughout this exploration) now reads `mode: Edge` — preserving exactly
    the look they said they liked, not reverting their tuning (only the field name/variant
    changed to track the rename). `examples/styles/barrier-wavy-volume.fmstyle.ron` now reads
    `mode: FullWave`, demonstrating the corrected always-has-volume look.
- **Glow now composes on edge distance, not center distance**, so the halo hugs the wavy edge
  instead of clipping against an invisible flat line: `edge_dist = max(max(top_edge - y, y -
  bottom_edge), 0.0)`, then `glow_alpha = 1 - smoothstep(0.0, glow_radius, edge_dist)`. Re-derived:
  when `wave == 0`, `edge_dist` reduces algebraically to `max(vertical_dist - half_thickness, 0)` —
  the old center-distance formula's argument — so by smoothstep's shift-invariance this is an exact
  no-op when wavy is off (glow-alone, unaffected) and composes correctly with wavy when both are on.
- **Sample style**: `examples/styles/barrier-wavy.fmstyle.ron` (Glow + a moderate `WavySpec`,
  amplitude 6px/wavelength 220px/speed 18) added via the existing `dump_sample_styles.rs`
  generator convention; the three pre-existing sample files each needed a `wavy: None,` line
  inserted next to their existing `pulse: None,` (targeted edit, not wholesale regeneration — same
  reasoning Phase F used: the generator's current output has drifted slightly from the checked-in
  files on unrelated fields, so a full overwrite would pick up unrelated diffs).
- **Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), and `cargo test --workspace` all
  clean (`shipped_sample_styles_parse` now finds 4 sample files, still `>= 3`). **Not yet manually
  run** (per the "never run the app yourself" rule) — worth confirming, next time someone has hands
  on the app: with no `wavy` set, the barrier looks identical to before at various
  thickness/Line/Glow combinations; importing `barrier-wavy.fmstyle.ron` makes the top edge ripple
  calmly (not one obvious repeating wave, not jittery), the ripple continues during playback and
  freezes exactly on pause/scrub-stop, and the glow (Glow kind) follows the ripple continuously
  rather than clipping against a flat edge; and that an exported clip's ripple is deterministic
  frame-to-frame at a given timestamp versus interactive playback at the same timestamp.

**Phase H — Elliptical, radiating-glow flash — DONE.** Plan:
`~/.claude/plans/the-most-recent-changes-delightful-rabbit.md`.

- **Schema** (`crates/project/src/style.rs`): `FlashSpec.radius_px: f32` → `radius_x_px: f32,
  radius_y_px: f32` — a clean breaking rename (pre-1.0 format, no back-compat shim), confirmed with
  the user beforehand.
- **Renderer** (`crates/render/src/effects.rs`): `EffectInstance.radius: f32` → `radius: [f32; 2]`,
  shared by particles (circular: `[size_px, size_px]`) and flashes (elliptical: `[radius_x_px,
  radius_y_px]`); its `wgpu::vertex_attr_array!` entry moved from `Float32` to `Float32x2` (shader
  locations shifted: `color` is now location 4, and a new `softness` field occupies location 5 —
  see below). `Flash` gained `radius_x_px`/`radius_y_px` (was one `radius_px`); `spawn_flash` sets
  both from the spec.
- **Ellipse shape** (`crates/render/src/effects.wgsl`): `Instance.radius` became `vec2<f32>` with
  **no other shader-math change needed** — WGSL's `*` between two `vec2<f32>` is already
  component-wise, so `pixel = center + local * radius` was already correct once `radius` was
  retyped. Verified rather than assumed: `out.local` is the pre-scale unit-quad coordinate, and
  since that expression is affine in `local`, the interpolated `in.local` at any fragment already
  equals `(pixel - center) / radius` component-wise, so `length(in.local)` is already the correct
  elliptical-normalized radius once `radius.x != radius.y` — no special-casing needed in `fs_main`
  for the ellipse itself.
- **Glow/radiate shape fix**: the old falloff (`1 - smoothstep(0.6, 1.0, d)`) was solid/opaque out
  to 60% of the radius and only softened in the outer 40%, which read as a flat disc rather than
  light radiating outward — true for both particles and flashes previously, but only flashes needed
  fixing (particles' hard-edged-dot look is correct and was preserved exactly). Added
  `EffectInstance.softness: f32` (0.0 = today's hard-edged dot, particles; 1.0 = full-radius
  radiating glow, flashes) as an interpolation knob rather than a bool, so a future style axis could
  expose partial values without another shader change. `fs_main` now blends
  `hard_edge = 1 - smoothstep(0.6, 1.0, d)` (unchanged) with `soft_glow = pow(clamp(1-d, 0, 1),
  1.6)` via `mix(hard_edge, soft_glow, softness)` — `softness == 0.0` makes `mix` select
  `hard_edge` exactly, so particle rendering is pixel-identical to before this phase; only flashes
  (`softness == 1.0`) move to the new soft-glow curve. The `1.6` exponent is a tune-by-eye starting
  point, not derived from anything — flag as the first thing to adjust if the glow looks too
  soft/sharp once seen rendered.
- **Samples**: `crates/project/examples/dump_sample_styles.rs`'s `sparks` example updated to
  `radius_x_px: 40.0, radius_y_px: 40.0` (kept equal, pixel-identical look), regenerated and the
  `radius_px` line in `examples/styles/sparks.fmstyle.ron` hand-edited to the two new fields
  (targeted edit, not a wholesale regenerate-and-overwrite — same convention Phases F/G already
  used, since the generator's current output has drifted from the checked-in files on unrelated
  fields). New `examples/styles/ellipse-flash.fmstyle.ron` (`TransitionKind::Flash`, `radius_x_px:
  70.0, radius_y_px: 20.0`) ships alongside it so there's a genuinely elliptical example to
  visually confirm, per the plan's suggestion.
- **Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), and `cargo test --workspace` all
  clean (`shipped_sample_styles_parse` now finds 5 sample files). **Not yet manually run** (per the
  "never run the app yourself" rule) — worth confirming, next time someone has hands on the app:
  re-importing `sparks.fmstyle.ron` looks pixel-identical to before (particles unchanged, flash now
  visibly glows/radiates rather than reading as a flat disc); importing `ellipse-flash.fmstyle.ron`
  shows a visibly stretched (non-circular) flash that still reads as a soft radiating glow, not a
  solid ellipse; and if the `1.6` falloff exponent looks off, that's the value to tune first.

**Phase I — Continuous "grinding" particles + sustained flash-as-glow — DONE.** Plan:
`~/.claude/plans/the-most-recent-changes-delightful-rabbit.md`. **Phase J (docs) is now done** —
see `docs/fmstyle-format.md` for the field-by-field spec.

- **Unified `HitEvent` into a richer `NoteInterval`** (`crates/render/src/notes/mod.rs`): the old
  `HitEvent { time_seconds, x_px }` (attack instant + lane-center x) only had enough information
  for one-shot spawns. Replaced with `NoteInterval { start_seconds, end_seconds, x_left, x_right }`
  (plus an `x_center()` helper) built in the same `rebuild_instances` loop that already computed
  `note_x`/`key.width()`/`duration` for the note instances — `Loaded.note_intervals` replaces
  `Loaded.hit_events`, and `NotesRenderer::note_intervals()` replaces `hit_events()` 1:1.
  `Compositor::update_transition` (`crates/render/src/lib.rs`) needed only its one internal call
  site updated (`self.notes.hit_events()` → `self.notes.note_intervals()`) — its own public
  signature, and both its callers in `app/src/main.rs`/`crates/export/src/lib.rs`, are unchanged.
  `barrier::BarrierRenderer`'s separate `note_start_times()`-based pulse (Phase D) is a different,
  untouched accessor.
- **Continuous particle emission** (`project::EmissionMode`, new enum on `ParticleSpec`):
  `Burst` (default, today's one-burst-per-arrival behavior, unchanged) or
  `Continuous { rate_per_second }` (particles spawned every frame a note is held, spread across
  the note's *key width* rather than its center point). `crates/render/src/effects.rs` extracted
  the existing per-burst spawn loop into a shared `spawn_one_particle` helper (`spawn_particles`,
  the burst entry point, is now a thin loop calling it `count` times — behavior-preserving
  refactor) so continuous emission's per-frame spawn calls (a Poisson-style fractional-count draw,
  `expected = rate_per_second * dt`, floor + a random round-up for the fractional remainder) can
  reuse the identical spawn math. Continuous emission uses **a plain point-in-time containment
  check** (`interval.start_seconds <= time_seconds <= interval.end_seconds`), scanning the whole
  `note_intervals` slice every ordinary step — deliberately not the crossing/binary-search check
  the burst spawn uses, since burst is a one-shot cue that must not be missed while continuous
  emission is a per-frame density sample where a note too short to visibly register as "held"
  needs no special-casing anyway. `O(n)` per step, same "fine for MIDI-file-sized data, not worth
  amortizing" tradeoff already made elsewhere in this codebase (e.g. the barrier pulse's own
  linear-scan alternatives).
- **Sustained flash** (`project::FlashMode`, new enum on `FlashSpec`): `Instant` (default, today's
  behavior — decays over `decay_seconds` starting immediately at note-on) or `Sustained` (holds at
  full intensity for as long as the note is held, decaying only after the note ends) — the "glow
  triggered by a key press" look the user called out as central to the seemusic/Synthesia
  aesthetic. **No repeated per-frame spawning needed, unlike continuous particles**: `Flash` was
  reworked from an incrementally-aged `age_seconds` counter to an absolute
  `decay_start_seconds` threshold, chosen once at spawn time (`time_seconds` for `Instant`, the
  note's already-known `interval.end_seconds` for `Sustained`) and never touched again. Alpha
  became a pure function of current transport time:
  `elapsed = (time_seconds - flash.decay_start_seconds).max(0.0); t = 1.0 - (elapsed /
  decay_seconds).clamp(0.0, 1.0)` — for `Instant` this is identical to the old curve (elapsed
  grows immediately from spawn); for `Sustained`, `decay_start_seconds` sits in the future at
  spawn time, so `elapsed` stays clamped to `0` (full intensity) for the note's whole held
  duration and only starts counting up once the note actually ends, at which point it decays
  along the exact same curve `Instant` always used. Expiry (`retain`) became
  `time_seconds - flash.decay_start_seconds < flash.decay_seconds` — correctly keeps a
  still-held `Sustained` flash alive (LHS negative/clamped) with no extra branching. This
  parameterize-not-special-case shape mirrors `BlackKeyFill::Auto`/`Fill::Solid` elsewhere in this
  codebase. `rebuild_instances` (which builds the per-frame instance list from the pool) needed
  `time_seconds` threaded in as a new parameter, since flash alpha is no longer computed from a
  value already stored per-flash.
- **Wiring**: no changes needed in `app/src/main.rs`/`crates/export/src/lib.rs` — both new knobs
  are read entirely inside `effects::EffectsRenderer::update`/`rebuild_instances` from data
  (`ParticleSpec`/`FlashSpec`/`NoteInterval`) those call sites already pass through unchanged.
- **Examples**: `dump_sample_styles.rs`'s `sparks` example needed explicit
  `emission: EmissionMode::Burst`/`mode: FlashMode::Instant` added to its literals (required even
  though both fields are `#[serde(default)]`, since Rust struct literals don't get deserialization
  leniency) — `sparks.fmstyle.ron`/`ellipse-flash.fmstyle.ron` got the same two lines hand-inserted
  (targeted edit, not a wholesale regenerate — same convention every prior phase in this milestone
  used, since the generator's current output has drifted from the checked-in files on unrelated
  fields in some of them; these two files happened to still match exactly, but the edit was still
  done as a targeted insert for consistency). Two new samples ship to concretely demo each new
  look: `examples/styles/grinding-particles.fmstyle.ron` (`TransitionKind::Particles`,
  `Continuous{rate_per_second: 40.0}`, small/short `size_px`/`lifetime_seconds` so it reads as
  streaming grit rather than sparks) and `examples/styles/key-glow.fmstyle.ron`
  (`TransitionKind::Flash`, `FlashMode::Sustained`, a soft elliptical shape with a gentle 0.6s
  release decay, no particles) — the latter is the one that most directly demonstrates the
  "glow triggered by the key press" look.
- **Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), and `cargo test --workspace` all
  clean (`shipped_sample_styles_parse` now finds 8 sample files). **Not yet manually run** (per the
  "never run the app yourself" rule) — worth confirming, next time someone has hands on the app:
  `Burst`/`Instant` (no fields set) still looks identical to before (one burst, one quick decaying
  flash per arrival); importing `grinding-particles.fmstyle.ron` shows particles streaming across
  the *width* of each held key, stopping exactly when the note ends, with a longer-held note
  visibly producing more particles than a short one at the same rate; importing `key-glow.fmstyle.ron`
  shows the glow appearing at note-on, holding at full brightness for the entire held duration (not
  a quick pulse), and only fading over `decay_seconds` after release — confirm a long-held note
  stays glowing throughout and a short note's glow barely shows before decaying, i.e. glow duration
  genuinely tracks note length rather than being a fixed-length flash; and that scrubbing
  (forward/backward, including mid-burst/mid-glow) doesn't leave stuck particles or a frozen glow,
  relying on the existing scrub-clears-the-pool behavior unchanged by this phase.

### Phase K: Glow brightness/overexposure knob (separate plan, continues the lettered-phase scheme)

Full plan: `~/.claude/plans/for-the-current-plan-abstract-liskov.md`. Testing Phases A-J, the user
noticed every "glow" effect (note glow, barrier glow, barrier pulse, flash, particles) only ever
had a **radius** and an alpha-only **intensity** — turning intensity up made a halo more opaque,
never more *overexposed* like a real light source blowing out to white. Phase K adds a
`brightness: f32` knob to all of them (default `1.0`, exact no-op), unified via one mechanism: the
effect's linear color is multiplied by `brightness` *before* it's blended in. The preview/export
render target is non-HDR 8-bit (`Rgba8Unorm`), so a channel pushed past `1.0` clamps to white on
write — a physically-motivated "blown out highlight" look with no bloom pass, no tone-mapping, no
new render target format. This is documented in full, field-by-field, in
`docs/fmstyle-format.md`'s new "Brightness/overexposure" section — this CLAUDE.md section is the
narrative of what changed and why, not a duplicate spec.

- **`BarrierLayer` unified onto the shared `Glow` struct, a breaking schema change** (same
  precedent as Phase H's `FlashSpec` rename): dropped `kind: BarrierKind` + `glow_radius_px: f32`,
  gained `glow: Option<Glow>` — presence of a `Glow` *is* the on/off switch now, matching
  `NoteLayer::glow`'s existing pattern exactly. `BarrierKind` was deleted entirely (its only
  consumer, `barrier.rs`'s `matches!(kind, BarrierKind::Glow)`, became `glow.is_some()`).
  `Style::from_legacy`'s `BarrierLayer` literal sets `glow: None` — barrier glow was never
  reachable from the legacy quick-control sliders (`BarrierStyle` only has `color`/`thickness`),
  so this is an exact no-op for every project without an imported style.
- **`Glow` gained `brightness: f32`** (`#[serde(default = "default_brightness")]`, `1.0`) — shared
  by `NoteLayer::glow` and (new) `BarrierLayer::glow`. `Glow::default()` sets `intensity: 1.0` too
  (it had no `Default` impl before this phase) so a migrated `kind: Glow` barrier style that
  doesn't set `intensity` explicitly keeps the same resting/peak brightness the old hardcoded
  `0.35`/`0.65` shader constants gave.
- **`Pulse` gained `brightness: f32`** (`#[serde(default = "default_pulse_brightness")]`, `1.6` —
  a documented tune-by-eye default, same convention as `effects.wgsl`'s existing `1.6` soft-glow
  exponent), and its shader composition changed from `mix(color, white, pulse * 0.5)` (capped at a
  50% blend) to `color * mix(1.0, brightness, pulse)` (genuine overexposure, unbounded). This is an
  **intentional visual change** to `Pulse`'s look, not required to stay pixel-identical — `Pulse`
  is only reachable via an imported style, never the legacy sliders, so there's no default-project
  regression risk.
- **`FlashSpec`/`ParticleSpec` each gained a flat `brightness: f32`** (default `1.0`) — no nested
  `Glow` struct, since neither's existing `color`/`radius`/`decay` shape matches `Glow`'s. Both
  already bake their final linear color into a GPU instance (`EffectInstance`) at spawn time
  (`crates/render/src/effects.rs`), so `brightness` is a pure CPU-side multiply applied once at
  spawn (`spawn_flash`/`spawn_one_particle`) — no `effects.wgsl` change needed for either.
- **Note glow** (`crates/render/src/notes/pipeline.rs`/`shader.wgsl`): `StyleUniform.glow_intensity`
  already had unused `y/z/w` slots (`[intensity, 0.0, 0.0, 0.0]`) — `brightness` now lives in `.y`,
  no new uniform field needed. `shader.wgsl`'s glow composition changed from
  `mix(style_uniform.glow_color_radius.xyz, fill_color, base_alpha)` to
  `mix(glow_color_radius.xyz * glow_intensity.y, fill_color, base_alpha)` — re-derived by hand (per
  this codebase's standing rule for shader-math changes) that `brightness = 1.0` reproduces the old
  line exactly, since multiplying by `1.0` is the identity.
- **Barrier glow** (`crates/render/src/barrier.rs`/`barrier.wgsl`): the glow halo previously always
  reused the bar's own core color (`uniforms.color_glow_radius.rgb`) with hardcoded `0.35 + 0.65 *
  pulse` resting/peak alpha and no intensity knob at all. `Uniforms` gained two new all-vec4 fields
  (`glow_style`: glow color xyz + intensity w; `glow_brightness_pulse`: x = glow brightness, y =
  pulse brightness) — same std140-safe all-vec4 convention every uniform in this codebase already
  follows, so there's no column-padding mismatch to get wrong. The glow's color, intensity, and
  brightness are now all independent of the bar's own color — `set_style` writes them from
  `barrier_layer.glow`, defaulting every glow field to a neutral value (`flags[0] = 0`, `glow_style
  = [0;4]`, `glow_brightness_pulse[0] = 1.0`) when `glow` is `None`. Re-derived by hand that
  `glow.intensity = 1.0`, `glow.brightness = 1.0`, and `glow.color == bar color` reproduces the
  pre-Phase-K look exactly: `glow_intensity` multiplies the same `0.35 + 0.65 * pulse` curve the
  shader always used, and `brightness = 1.0` leaves the glow color unmultiplied.
- **Sample styles**: `crates/project/examples/dump_sample_styles.rs`'s `barrier_pulse`/
  `barrier_wavy`/`barrier_wavy_volume` literals reworked from `kind: BarrierKind::Glow,
  glow_radius_px: R` to `glow: Some(Glow { color: <same as bar color>, radius_px: R, intensity:
  1.0, brightness: 1.0 })` (an exact-no-op migration), plus `brightness: 1.0`/`1.6` added to every
  `Glow`/`Pulse`/`FlashSpec`/`ParticleSpec` literal in the generator (required in Rust struct
  literals even though every field is `#[serde(default)]` for RON parsing — same gotcha every
  prior phase in this milestone hit). The eight checked-in `examples/styles/*.fmstyle.ron` files
  were hand-edited with the same targeted-insert convention prior phases used (not a wholesale
  regenerate-and-overwrite, since the generator's output has drifted from some checked-in files on
  unrelated fields) — `barrier-pulse.fmstyle.ron`/`barrier-wavy.fmstyle.ron`/
  `barrier-wavy-volume.fmstyle.ron` got the full `kind`/`glow_radius_px` → `glow: Some(...)`
  migration plus `barrier-pulse`'s `pulse.brightness: 1.6`; every other file's `barrier: (kind:
  Line, ...)` block became `barrier: (glow: None, ...)`; every `Glow`/`FlashSpec`/`ParticleSpec`
  literal gained its `brightness: 1.0` line.
- **Tests**: `crates/project/src/style.rs` gained `barrier_layer_with_glow_and_pulse_brightness_
  round_trips`, `barrier_layer_without_glow_round_trips`, `transition_layer_brightness_fields_
  round_trip` (RON round-trips covering the new `BarrierLayer` shape and every new `brightness`
  field), and `glow_without_brightness_field_loads_with_default`/`pulse_without_brightness_field_
  loads_with_tuned_default` (confirm `serde(default)` backfills `1.0`/`1.6` when an older-schema
  `Glow`/`Pulse` RON fragment omits the field entirely).
- **Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), and `cargo test --workspace` all
  clean (`shipped_sample_styles_parse` still finds all 8 sample files, now migrated to the new
  `BarrierLayer` shape). **Not yet manually run** (per the "never run the app yourself" rule) —
  worth confirming, next time someone has hands on the app: every existing sample style (after the
  mechanical `brightness: 1.0` migration) looks pixel-identical to before this phase; bumping a
  `Glow.brightness` (note and/or barrier) well past `1.0` (e.g. `3.0`-`8.0`) visibly washes the
  glow's core toward solid white while its outer falloff stays tinted; bumping `FlashSpec.
  brightness`/`ParticleSpec.brightness` on `key-glow.fmstyle.ron`/`grinding-particles.fmstyle.ron`
  shows the same overexposure look; and re-importing `barrier-pulse.fmstyle.ron` shows pulses
  flashing visibly whiter/brighter than before (previously capped at a 50% blend) — tune the
  sample's `Pulse.brightness` if the shipped `1.6` looks off, it's an explicitly flagged "tune by
  eye" constant.

### Phase L: white-hot core + natural corona redesign, `intensity` removed (follow-up to Phase K)

User feedback after Phase K landed: high `brightness` didn't read as "very white, radiant, like
looking at something white-hot" with a natural corona — it only ever brightened/recolored the
*halo*, never the glowing surface's own core, so it looked like an afterthought stuck to the
edges rather than the object itself heating up. Also flagged: `Glow`/`Pulse`/`FlashSpec` each had
both an `intensity` and a `brightness` knob doing overlapping jobs. Full plan: no separate doc,
implemented directly per user request in one session.

- **`intensity: f32` removed from `Glow`, `Pulse`, and `FlashSpec`** (`ParticleSpec` never had
  one; `Sheen::intensity` is a distinct, unrelated axis — left alone). Redundant once brightness
  is the sole "how strong" knob: `Glow::intensity` only ever scaled halo opacity (folded into the
  new radiating-falloff shape below); `Pulse::intensity` was a 0..1 peak-amplitude multiplier into
  `brightness` (removed — `brightness` alone is now the peak, so `intensity: 0.8, brightness: 1.6`
  becomes `brightness: 1.28`, or whatever peak is wanted); `FlashSpec::intensity` was peak alpha,
  which for an always-additive flash has the *exact same visual effect* as `brightness` (both just
  scale the additive contribution) — a flash is now always fully "on" at spawn/hold-start, fading
  to 0 over `decay_seconds` as before, with `brightness` alone controlling how hot it looks.
- **New shared mechanism, `hot_color(base, brightness)`**: at `brightness <= 1.0`, a plain dimmer
  (`base * brightness`); above `1.0`, desaturates toward pure white via
  `mix(base, vec3(1.0), 1.0 - 1.0/brightness)` — chosen specifically because multiplying a color's
  channels up (Phase K's approach) does **not** converge to white unless the channels already
  share the same magnitude (e.g. `[1.0, 0.3, 0.1] * 3.0` clips to `[1.0, 0.9, 0.3]`, a more
  saturated orange, not white); mixing toward white does, unconditionally. `brightness == 1.0` is
  an exact no-op on both branches (`base*1.0 == base`, `mix(base, white, 0.0) == base`), verified
  algebraically rather than assumed, same standing rule this file has held every prior shader-math
  change to. Implemented three times — verbatim WGSL in `barrier.wgsl` and `notes/shader.wgsl`
  (each a separate shader module, same "duplicate the small helper" convention `srgb_to_linear`
  already used three times in this codebase), and as a CPU-side equivalent `hot_color` in
  `crates/render/src/effects.rs` for particles/flashes (which bake their final color into a GPU
  instance at spawn time rather than reading a shader uniform per-fragment).
- **`corona_reach_scale(brightness) = 1.0 + 0.5 * (1.0 - 1.0/max(brightness, 1.0))`**: the halo's
  effective reach grows up to 1.5x the configured `radius_px` as brightness increases without
  bound — a real light source radiates further as it intensifies, not just a static-radius ring.
  `brightness <= 1.0` gives an exact `1.0` (no-op reach). Only applies to `barrier.wgsl`/
  `notes/shader.wgsl` (flash/particle radii are already explicit, user-set values with no
  brightness-driven reach — flashes were called out as "mostly fine" and left alone here).
- **Halo falloff changed from a flat-opacity smoothstep band to the natural radiating `pow` curve
  `effects.wgsl` already used for flashes** (`pow(1.0 - normalized_distance, 1.6)`), in both
  `barrier.wgsl` and `notes/shader.wgsl` — this is the "use the same concept everywhere" part of
  the ask: flashes already looked like light radiating outward (their `softness = 1.0` path), so
  the note/barrier halos were changed to match that shape instead of inventing a fourth one.
  Barrier's old resting/peak opacity blend (`0.35 + 0.65 * pulse`, from Phase D) is gone entirely —
  superseded by `hot_color`/`corona_reach_scale` both already being driven by the same
  pulse-blended `effective_brightness`, so the corona's "pulse response" now comes from getting
  whiter and reaching further, not from a separate opacity ramp.
- **The core itself now goes white-hot, not just the halo** — the actual fix for "looks like an
  afterthought stuck to the sides": `barrier.wgsl`'s `fs_main` now computes
  `color = hot_color(uniforms.color_glow_radius.rgb, effective_brightness())` for the bar's own
  fill (previously the core only ever got the old `mix(1.0, pulse_brightness, pulse)` — pulse
  could brighten it, but a resting `Glow::brightness` never touched the core at all, only the
  halo). Symmetrically, `notes/shader.wgsl`'s `fs_main` now computes
  `hot_fill = hot_color(fill_color, brightness)` and blends `hot_fill` (not the plain `fill_color`)
  under the halo — satisfying the "this idea should be applied for the notes too" ask. Both quads'
  vertex-shader inflation margins were widened from a flat `radius_px` to
  `radius_px * corona_reach_scale(brightness)`, computed identically in both stages (a shared
  `effective_brightness()`/`corona_reach_scale()` WGSL function per module) so the rasterized quad
  is always exactly large enough for the corona's current reach, no more and no less.
- **`Uniforms`/`StyleUniform` field renames, no layout/size change**: `barrier.rs`'s
  `glow_style.w` (used to carry `Glow::intensity`) is now unused/zeroed; `glow_brightness_pulse`
  keeps its `[resting_brightness, peak_brightness, 0, 0]` shape but `peak_brightness` now defaults
  to *equal* `resting_brightness` when no `Pulse` is configured (previously defaulted to `1.0`
  flat) — needed so `mix(resting, peak, 0)` degenerates to `resting` exactly rather than
  potentially un-doing a nonzero `Glow::brightness` when there's no pulse to drive `peak` away
  from `1.0`. `notes/pipeline.rs`'s `StyleUniform.glow_intensity: [f32;4]` (`[intensity,
  brightness, 0, 0]`) renamed to `glow_params: [f32;4]` (`[brightness, 0, 0, 0]`) — same all-vec4
  layout convention, one fewer meaningful slot.
- **Samples/tests**: every shipped `examples/styles/*.fmstyle.ron` file and
  `crates/project/examples/dump_sample_styles.rs`'s generator had their `intensity:` lines removed
  (targeted deletion, not regeneration — same convention every prior phase in this milestone used,
  since the generator's output has drifted from some checked-in files on unrelated fields);
  `Sheen::intensity` lines were left untouched (different field, still exists). `style.rs`'s tests
  updated to drop `intensity` from every `Glow`/`Pulse`/`FlashSpec` literal and RON fragment.
- **Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), and `cargo test --workspace` all
  clean. **Not yet manually run** (per the "never run the app yourself" rule) — worth confirming,
  next time someone has hands on the app: every sample style still looks reasonable at its shipped
  `brightness` values (no longer required to be pixel-identical to pre-Phase-L, since the halo
  falloff shape itself changed); pushing `Glow.brightness`/`Pulse.brightness` on `barrier-pulse.
  fmstyle.ron` or `gradient-glow.fmstyle.ron` well past `1.0` (e.g. `3.0`-`6.0`) now visibly turns
  the *core* of the bar/note white-hot (not just the halo around it) with a soft corona that
  reaches a bit further than at `brightness = 1.0`; and that `brightness <= 1.0` still behaves as a
  plain dimmer with no whitening, matching the documented no-op/dimmer boundary.

### Vendored note pipeline, pixel-parity (Phase B of the `.fmstyle.ron` milestone)

Phase B replaces the `neothesia_core`-backed `MidiOverlay` with an in-tree note-highway renderer,
per the plan's call to vendor it the same way `mp4-encoder` was forked from `ffmpeg-encoder` —
done specifically so `barrier_fraction` could become a real shader uniform instead of the fragile
viewport-remapping hack documented (and now deleted) above. **No visual change from before** —
this phase is pixel-parity, proven by re-deriving the math (not just eyeballing), same as every
prior barrier-related change in this file.

- **New module `crates/render/src/notes/`** (`mod.rs`/`pipeline.rs`/`instance.rs`/`shader.wgsl`)
  replaces `crates/render/src/midi_overlay.rs` entirely. `NotesRenderer` (was `MidiOverlay`) keeps
  the exact same public surface `Compositor` already called (`new`/`loaded_name`/
  `note_start_times`/`load`/`resize`/`render`), so `crates/render/src/lib.rs` only needed
  `mod midi_overlay` → `mod notes` and field renames — `app/src/main.rs`'s call sites are
  untouched except `update_midi`, which now needs a `&wgpu::Queue` argument (see below). Exported
  `GpuHandles` (same shape: borrowed `instance`/`adapter`/`device`/`queue`/`texture_format`) moved
  from `midi_overlay` to `notes` but is otherwise unchanged, so `app::gpu_handles` and
  `export::run_inner` (the two places that build one) needed no changes at all.
- **`crates/render/Cargo.toml` dropped the `neothesia-core` git dependency entirely** and added
  `piano-layout` (same pinned rev) as a *direct* dependency — exactly the situation CLAUDE.md's
  Neothesia-reuse section flagged as the trigger for doing this ("add direct wgpu-jumpstart/
  piano-layout deps... only when a future crate needs them without going through
  neothesia_core"). `midi-file` stays a direct dependency, unchanged. Verified via `Cargo.lock`:
  zero `neothesia-core` entries, one `wgpu` entry (no duplicate-version risk), one `piano-layout`
  entry, one `midi-file` entry.
- **Own render pipeline, hand-rolled rather than reusing `wgpu_jumpstart`'s generic `Uniform`/
  `Instances`/`Shape` helpers** (`notes/pipeline.rs::NotesPipeline`) — same manual-wgpu-calls
  style `video_quad.rs` already used elsewhere in this crate, now applied here too since owning
  the shader removed the only reason to keep linking against Neothesia's renderer-side crate.
  Two uniform bind groups (view, time — same split as upstream: view changes on
  resize/calibration, time changes every frame) and one instance buffer that grows (doubling via
  `create_buffer_init`-style recreate, not amortized-growth — fine, MIDI files are not
  update-hot) whenever a MIDI file needs more instance slots than the current capacity.
- **Own `notes/shader.wgsl`**, forked from the vendored `neothesia-core/.../waterfall/pipeline/
  shader.wgsl` with exactly one behavioral change: `keyboard_y` is no longer hardcoded to
  `view_uniform.size.y / 5.0` (i.e. always 80% down) — it reads a new `barrier_fraction` field on
  `ViewUniform` directly (`keyboard_y = view_uniform.size.y * view_uniform.barrier_fraction`).
  Because `ViewUniform.size` is now always built from the *real* canvas size (no more feeding it
  a `virtual_height` that differs from what `set_viewport` gets), `builtin(position)` and the
  vertex shader's `note_pos`/`size` varyings are automatically in the same coordinate system at
  any `barrier_fraction` — the exact bug class the "notes fading out before reaching the barrier"
  section above had to work around now can't occur, because there's no second coordinate system
  to disagree with the first. `render::notes::NotesRenderer::render` now does only a
  `set_scissor_rect` (real canvas pixels, no `set_viewport` override at all) to clip notes past
  the barrier line — the whole `virtual_canvas_height`/`HARDCODED_HIT_LINE_FRACTION` apparatus
  and its long doc-comment derivation in the old `midi_overlay.rs` are gone.
- **`NoteInstance` gained `velocity: f32` and `track_index: f32`** (normalized 0.0-1.0 velocity,
  raw MIDI track index as a float since vertex attributes are all floats), per the plan's explicit
  ask to future-proof for `ColorBinding::ByVelocity`/`ByTrack` (`project::style`) — both fields
  are populated when instances are built but **not read anywhere in the v1 shader**, matching the
  plan's "cheap, future-proofs... even though v1 ignores them in-shader" framing.
- **Instances are built directly in `NotesRenderer::rebuild_instances`** (was
  `WaterfallRenderer::resize`'s internal loop) — same algorithm, ported rather than changed:
  filter notes to the standard 88-key range and non-drum channel, sort by start time (newer notes
  draw on top, matching Neothesia's own convention), look up each note's `piano_layout::Key` for
  x/width/sharpness, and combine the calibrated left-offset + roundedness directly into each
  instance's `position`/`radius` at construction time (previously a second pass,
  `apply_note_adjustments`, mutated already-built instances after the fact — folding it into the
  single construction loop is a minor simplification enabled by no longer needing a
  `piano_layout`-agnostic upstream method signature to work around). The sRGB→linear color
  conversion (`color_to_linear`) is copied verbatim from `wgpu_jumpstart::Color::into_linear_rgb`
  (same source, credited in a doc comment) since that's the one small piece of math actually worth
  keeping rather than re-deriving.
- **`Compositor::update_midi` and `NotesRenderer::update` now take a `&wgpu::Queue` argument**
  (previously the old `MidiOverlay` didn't need one at the call site, since `WaterfallRenderer`
  cloned and kept its own `wgpu::Queue` internally). Both of this phase's two call sites
  (`app/src/main.rs::update_midi_position`, `crates/export/src/lib.rs`'s render loop) already had
  a `Gpu`/`gpu` in scope, so this was a one-line change at each — same pattern
  `update_viewport`/`upload_frame` already used.
- **Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), and `cargo test --workspace` all
  clean; the vertex-shader math (barrier line position, note fall trajectory, rounded-rect
  distance field) was re-derived by hand against the original vendored shader line-by-line rather
  than assumed correct from a clean build — `cargo build`/`clippy` cannot catch a
  wrong-but-type-correct shader port, per this file's own repeated caution on shader-side bugs
  elsewhere. **Not yet manually run** (per the "never run the app yourself" rule) — worth
  confirming, next time someone has hands on the app, that a loaded MIDI file's notes fall,
  clip at the barrier, and are colored exactly as before at a few different `barrier_fraction`
  values (including far from the 0.8 default, which is what previously exposed the fade-out bug),
  since this phase touches the same code path that bug lived in.

### Barrier glow/pulse pass (Phase D of the `.fmstyle.ron` milestone) — DONE

Phase D promotes the barrier from a plain `egui` overlay (milestone 6a) to a real wgpu render
pass, so it now shows up in exported video too — and reads `project::BarrierLayer`'s
`kind`/`color`/`thickness`/`glow_radius_px`/`pulse` fields instead of just the legacy
`BarrierStyle`'s color/thickness.

- **New module `crates/render/src/barrier.rs`** (+ `barrier.wgsl`), structured like
  `video_quad.rs`: no vertex buffer, six hardcoded unit-quad corners positioned/sized in the
  vertex shader from a uniform, one bind group. `Compositor` gained a `barrier:
  barrier::BarrierRenderer` field, constructed unconditionally in `Compositor::new` (no
  `BarrierLayer` needed at construction time, unlike `notes::NotesRenderer` — see below for why).
  Render order is now video quad → notes → **barrier** (`Compositor::render`), so the bar draws
  on top of falling notes, matching how the old egui overlay always painted on top of everything.
- **Barrier params are cheap uniform writes, not a dirty-checked rebuild** — unlike
  `NoteLayer`'s fill/sheen/glow (baked into `NoteInstance`s at build time, needing a full
  `compositor.resize`), every `BarrierLayer` field only drives a handful of uniform floats. So
  `Compositor::update_barrier` is called *unconditionally every redraw* (`app/src/main.rs`'s
  `apply_post_ui_updates`, right after the existing `update_viewport` call; `crates/export`'s
  render loop, right after its own `update_viewport`/`update_midi` calls) — the same treatment
  `update_viewport` already gets, no `applied_barrier_layer` dirty-check field needed at all.
- **Geometry/color** (`BarrierRenderer::set_style`): a full-canvas-width bar centered at
  `canvas_height * barrier_fraction`, thickness in canvas pixels (not on-screen/logical UI
  points like the old egui bar was) — so at a given `thickness`, how thick the bar reads on
  screen now depends on how much the preview image is scaled to fit the panel, same as the
  falling notes always did. This is an intentional consequence of the bar now living in the same
  canvas-pixel coordinate space export renders in, not a bug.
- **Glow** (`BarrierKind::Glow`) uses the exact same "inflate the rasterized quad by the glow
  radius, zero margin when disabled" trick `notes/shader.wgsl`'s note glow already uses — see
  `barrier.wgsl`'s `glow_margin`/`half_extent`. `BarrierKind::Line` (what `Style::from_legacy`
  produces, so a project with no imported style behaves exactly as before) leaves `flags.x`
  (glow_enabled) at 0 in the shader, matching the old flat-line look regardless of
  `glow_radius_px`.
- **Pulse** (`Option<Pulse>` — brightens on note arrival, decays over `decay_seconds`) is
  **stateless by design**, computed fresh every frame from the sorted note-onset list
  (`notes::NotesRenderer::note_start_times`, the same cached list the timeline's note-density
  strip already uses) rather than any spawned/tracked event queue: `BarrierRenderer::
  pulse_intensity` binary-searches (`partition_point`) for the most recent note start at or
  before the current (sync-offset-subtracted) transport time and linearly decays from
  `pulse.intensity` to 0 over `decay_seconds`. This works because a note's *leading edge* reaches
  the barrier exactly at `note.start` — re-derived from `notes/shader.wgsl`'s vertex math by
  hand: at `time == note.start` the position-offset term is exactly zero, leaving the quad's
  bottom edge sitting precisely at `keyboard_y`. Being stateless also means scrubbing anywhere
  (forward or backward) just recomputes correctly with no "clear on seek" bookkeeping — unlike
  Phase E's transition pass, whose particle pool *is* inherently stateful and will need that.
- **Scissor-rect gotcha**: `notes::NotesRenderer::render` (drawn immediately before barrier in
  the same render pass) leaves a scissor rect clipping to everything *above* the barrier line —
  wgpu scissor state persists across draw calls within one render pass until changed again, so
  `BarrierRenderer::render` must reset it to the full canvas before drawing, or the bar itself
  (which sits at/below that clip edge, and extends further below when glow is enabled) would be
  clipped away instead of rendered. Caught by re-deriving the render-pass state machine by hand,
  not by running the app — `cargo build`/`clippy` can't catch a wrong-but-type-correct scissor
  rect left over from a previous draw call in the same pass.
- **`ui::draw_barrier_handle` now only owns the drag hit-region** — the color/thickness-styled
  rect-fill and "barrier" text label it used to paint are gone (that's the compositor's job now);
  it keeps exactly the `Sense::drag()` + accumulated `drag_delta()` interaction that edits
  `calibration.barrier_fraction`, same pattern `draw_calibration_handles`/`draw_crop_handles` use.
- `Project::effective_barrier_layer()` mirrors `effective_note_layer()` exactly (imported style's
  `barrier` layer wins, else synthesized from `barrier_style`/`note_style` via
  `Style::from_legacy`); `app/src/main.rs` has its own free-function mirror
  (`effective_barrier_layer(&UiState)`) for the same reason `effective_note_layer`'s mirror
  exists (`UiState` isn't a `Project`).
- **Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), and `cargo test --workspace` all
  clean. **Not yet manually run** (per the "never run the app yourself" rule) — worth importing
  `examples/styles/barrier-pulse.fmstyle.ron` (kind: Glow, thickness 6px, glow_radius_px 24,
  pulse intensity 0.8 / decay 0.35s — already shipped, exercises every new code path at once)
  next time someone has hands on the app: confirm the bar glows continuously at rest, briefly
  flares brighter each time a note arrives then decays back over ~0.35s, and that dragging the
  barrier handle still moves it (now with no visible egui-drawn line under the cursor, since the
  rendered bar itself is what moves). Also worth exporting a short clip and confirming the barrier
  bar (previously invisible in exports) now appears baked into the output frame.

### Transition particles + flash pass (Phase E of the `.fmstyle.ron` milestone) — DONE

Phase E adds the last of the three visual axes from the milestone's original scope: a burst of
particles and/or a decaying radial flash spawned when a note arrives at the barrier, reading
`project::TransitionLayer`'s `kind`/`particles`/`flash` fields. This is the only one of the three
axes with genuine per-frame *state* (a particle's position is the integral of its velocity/gravity
since spawn) rather than something a `time_seconds` value alone can reproduce, unlike the barrier's
stateless pulse (Phase D) or the note fill's per-instance-baked look (Phase C).

- **Hit-event precompute lives in `render::notes`, not a new module** — `render::notes::HitEvent {
  time_seconds, x_px }` is built in `NotesRenderer::rebuild_instances` (`crates/render/src/notes/
  mod.rs`) in the exact same loop that builds `NoteInstance`s, since the x position needs the same
  calibrated keyboard layout the instances are built from (`key.x() + left_x + key.width() * 0.5`,
  the note's lane center). Stored on `Loaded` alongside the pre-existing `note_starts` (used by the
  timeline's density strip and the barrier's pulse) and exposed via a new `NotesRenderer::
  hit_events()` accessor — same "cached at rebuild time, sorted ascending since notes are already
  sorted by `note.start` for draw order" shape `note_starts` already used. Events are only built for
  the notes actually drawn (in-range, non-drum, same filter as instances) — a note that never
  renders never spawns a transition either.
- **New module `crates/render/src/effects.rs`** (+ `effects.wgsl`) owns `EffectsRenderer`: a CPU
  particle pool + a flash list, simulated on the CPU and re-uploaded as instanced quads every
  frame — structured like `notes/pipeline.rs` (own shader, own growable instance buffer(s)) but,
  unlike every other pass added so far, genuinely stateful across calls rather than a pure function
  of the current inputs.
- **Spawn/simulate model**: `EffectsRenderer::update(device, queue, canvas_size, barrier_fraction,
  transition_layer, time_seconds, hit_events)` tracks `last_time_seconds` and, on an *ordinary*
  step (`0.0 <= time_seconds - last_time_seconds <= MAX_ORDINARY_STEP_SECONDS`, `0.35`s), binary-
  searches `hit_events` for the slice crossed since the last call (`partition_point` twice, same
  technique `barrier::BarrierRenderer::pulse_intensity` already uses on `note_starts`) and spawns
  one burst per crossed event before advancing every live particle/flash by that step's `dt`.
  A step outside that range — including the very first call (`last_time_seconds == None`) — clears
  both pools instead of spawning every event a big jump skipped or trying to run particles
  backward: **transient effects have no well-defined mid-scrub state to reconstruct**, so a scrub
  (forward or backward) just starts the pool over, empty, at the new position. This is the
  documented tradeoff the plan called out ("scrubbing backward clears the pool") extended
  symmetrically to large forward jumps for the same reason.
- **Particles**: spawn spread around straight up (canvas convention is y-down, so "up" is negative
  y) within `±spread_degrees/2` of vertical, speed jittered `0.5x`-`1.0x` of `speed_px`, then
  integrate `pos += vel * dt; vel.y += gravity_px * dt` (gravity pulls particles back down) each
  step, fading alpha linearly over `lifetime_seconds` and expiring at zero. **Jitter uses a tiny
  hand-rolled deterministic xorshift32 PRNG** (`effects::Rng`), not a `rand` dependency — no crate
  in this workspace currently depends on `rand`, and a few lines of xorshift is enough for "looks
  plausibly random" without pulling in a new dependency for it.
- **Flashes**: a single quad per spawn at the hit position, alpha decaying linearly from
  `intensity` to 0 over `decay_seconds`, radius fixed at `radius_px` (no growth animation) —
  intentionally the simplest interpretation of `FlashSpec` that still reads as "a bright pop", left
  that simple rather than adding an expansion curve the schema doesn't ask for.
  - **No procedural texture asset**: `effects.wgsl`'s fragment shader computes a soft circular
    falloff (`1.0 - smoothstep(0.6, 1.0, length(local))`, `local` being the quad's -1..1
    center-relative coordinate) directly from the quad geometry, same "signed-distance math in the
    fragment shader instead of a sampled texture" style `notes/shader.wgsl`'s rounded-rect and
    `barrier.wgsl`'s glow falloff already use in this codebase.
- **Two blend modes, one shader**: `effects.wgsl`'s fragment shader always outputs *premultiplied*
  color (`rgb * alpha`, `alpha`), so `EffectsRenderer` can build two pipelines from the identical
  shader module differing only in `BlendState` — additive (`One, One` on both channels, for
  flashes and `ParticleSpec::additive = true` particles) and premultiplied-alpha (`One,
  OneMinusSrcAlpha`, for `additive = false` particles). Flashes always draw additive regardless of
  `ParticleSpec` (a flash reads as a bright pop either way; `FlashSpec` has no `additive` field of
  its own). **Simplification, documented rather than engineered around**: which pipeline a
  *particle* draws under is decided once per `update` call from the *currently resolved*
  `TransitionLayer.particles.additive`, not stored per-particle at spawn time — if a running
  project imports a different style mid-flight while particles from the old one are still alive,
  those leftover particles finish out under the new blend mode rather than the one they spawned
  under. Not worth the extra per-particle bookkeeping for an edge case (switching styles while
  particles are mid-flight) this milestone doesn't otherwise need to handle.
- **Render order**: video quad → notes → barrier → **effects** (`Compositor::render`), on top of
  everything else, matching the plan's specified order — a spark burst should visually sit above
  the barrier bar it's spawned from. `EffectsRenderer::render` resets the scissor rect to the full
  canvas defensively (barrier's own `render` already does this before it runs, but re-asserting
  costs nothing and doesn't depend on `Compositor::render`'s draw order never changing elsewhere).
- **Wiring**: `Compositor::update_transition` (new, mirrors `update_barrier`'s shape) pulls
  `self.notes.hit_events()` internally so callers don't need to thread them through — same pattern
  `update_barrier` already uses for `note_start_times()`. Called unconditionally every redraw in
  `app/src/main.rs::apply_post_ui_updates` (right after `update_barrier`, using the same
  `midi_time` already computed there) and once per output frame in `crates/export/src/lib.rs`'s
  render loop (right after its own `update_barrier` call) — export renders frames in strictly
  increasing `t` order at a fixed `1/fps` step, which is always well inside
  `MAX_ORDINARY_STEP_SECONDS`, so the sim behaves there exactly like ordinary interactive playback
  with no special-casing needed. `Project::effective_transition_layer()`
  (`crates/project/src/lib.rs`) and its `app/src/main.rs` free-function mirror
  (`effective_transition_layer(&UiState)`) follow the exact same "imported style wins, else
  synthesize via `Style::from_legacy`" shape as `effective_note_layer`/`effective_barrier_layer` —
  `Style::from_legacy` always produces `TransitionKind::None`, so a project with no imported style
  spawns nothing, matching the pre-Phase-E look exactly.
- **No new UI** — per the milestone's contract, "Import style…" is still the only surface; there
  are no sliders for particle count/speed/etc., only the `.fmstyle.ron` file format.
  `examples/styles/sparks.fmstyle.ron` (shipped back in Phase A) already exercises
  `TransitionKind::ParticlesAndFlash` with both a `ParticleSpec` and a `FlashSpec` populated, so no
  new sample style was needed for this phase.
- **Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), and `cargo test --workspace` all
  clean. **Not yet manually run** (per the "never run the app yourself" rule) — worth importing
  `examples/styles/sparks.fmstyle.ron` next time someone has hands on the app: confirm a burst of
  warm-colored sparks flies up and falls back down under gravity each time a note reaches the
  barrier, alongside a quick white flash, both fading out within well under a second; confirm
  scrubbing around (including scrubbing backward mid-burst) doesn't crash or leave stuck particles
  hanging in place; and confirm an exported clip bakes the bursts into the output frames at the
  right timestamps.

### Note fill effects: gradient, sheen, glow (Phase C of the `.fmstyle.ron` milestone) — DONE

Phase C makes the vendored note pipeline (Phase B) actually read `project::NoteLayer`'s
`fill`/`sheen`/`glow` fields instead of only `roundedness`/`fall_speed` — a vertical gradient fill,
a diagonal specular sheen stripe, and a soft outer glow, all driven by data, no new UI beyond the
existing "Import style…" button.

- **Effective-style wiring**: `render::Compositor::new`/`load_midi`/`resize` (and
  `render::notes::NotesRenderer`'s equivalents) now take `&project::NoteLayer` instead of
  `&project::NoteStyle`. Both `app` and `export` compute this the same way — added
  `Project::effective_note_layer(&self) -> NoteLayer` (`crates/project/src/lib.rs`):
  `self.style.clone().unwrap_or_else(|| Style::from_legacy(&self.note_style,
  &self.barrier_style)).notes.resolve(0.0).clone()`. `app/src/main.rs` has its own
  `effective_note_layer(&UiState) -> NoteLayer` free function doing the identical computation off
  `UiState` fields (can't call the `Project` method directly — building a whole `Project` just to
  resolve this would mean cloning video/MIDI paths for no reason). `AppState::applied_note_layer`
  replaces the old `applied_note_style` dirty-check field — comparing the *resolved* `NoteLayer`
  means a style import (which doesn't touch `note_style`/`barrier_style` at all) is caught by the
  exact same dirty check as a slider drag, one code path instead of two.
- **`NoteInstance` gained `color_bottom`** (`crates/render/src/notes/instance.rs`) alongside the
  existing `color` (renamed `color_top`) — a vertical-gradient fill is baked into each instance at
  build time as two endpoints rather than needing a second draw call or a per-fragment lookup into
  the style layer. For `Fill::Solid`, `color_top == color_bottom`, so the shader's gradient mix is
  unconditionally a no-op and the default (no imported style) look is pixel-identical to Phase B —
  this is what keeps `Style::from_legacy`'s output looking exactly like the pre-Phase-C sliders.
  `NotesRenderer::rebuild_instances` resolves `NoteLayer::fill` once per rebuild (not per note) via
  `ColorBinding::resolve_constant()`, then applies the existing sharp-key darkening (`* 0.6`) to
  both endpoints independently, same shape as the old single-`color` darkening.
- **Sheen and glow are style-wide uniforms, not per-note data** — a `StyleUniform` (new bind
  group 2 in `crates/render/src/notes/pipeline.rs`/`shader.wgsl`) carries fill kind
  (solid/gradient), sheen intensity/width/angle, and glow color/radius/intensity, uploaded once via
  `NotesPipeline::set_style` whenever `apply_view` runs (same call sites as `set_view`/`set_speed`).
  **Deliberately packed as four plain `vec4<f32>`s**, not a natural Rust-shaped struct with
  `vec3`/`f32`/`u32` fields — mirrors the milestone-4 lesson (documented above) that WGSL's
  std140-like uniform layout silently pads odd-sized fields (there it was `mat3x3<f32>`; here it
  would have been every `vec3<f32>` bumping to 16 bytes and every scalar needing manual trailing
  padding). All-vec4 sidesteps needing to reason about that padding at all.
- **Glow needs the rasterized quad to extend past the note's own box**, since the fragment shader
  can only paint pixels the rasterizer actually covers. `shader.wgsl`'s `vs_main` computes the
  note's true (unpadded) `position`/`size` first — fed to the fragment shader unchanged, so the
  rounded-rect distance field and gradient math are unaffected — then, only if `glow_enabled`,
  additionally inflates the *vertex transform's* position/size by `glow_radius_px` on all sides
  before applying `view_uniform.transform`. When glow is disabled the inflation margin is exactly
  `0.0`, making this an algebraic no-op (not just "visually close") — re-derived by hand rather
  than assumed, same standard this file has held every prior shader change to (the rotation-shear
  and barrier-fade-out bugs earlier in this file were both exactly this class of mistake slipping
  past `cargo build`/`clippy`).
- **Fragment shader composition order**: base fill (solid or gradient) → sheen (additive
  brightening along a fixed diagonal band, computed from the fragment's position relative to the
  note's true top-left, independent of any glow inflation) → glow (computed last, since it needs
  the already-composited fill color for `mix(glow_color, fill_color, base_alpha)` — glow_alpha is
  scaled by `(1 - base_alpha)` so it only shows outside/at the note's edge rather than washing out
  the note's own interior).
- **Not yet manually run** (per the "never run the app yourself" rule) — worth importing
  `examples/styles/gradient-glow.fmstyle.ron` (exercises all three: gradient + sheen + glow
  together) via the Project tab's "Import style…" button next time someone has hands on the app,
  scrubbing to where notes are visible, and confirming: notes show a top-to-bottom color blend, a
  diagonal bright stripe sweeps across each note, and a soft halo extends past each note's edges.
  Also worth confirming a project *without* an imported style still looks exactly like before this
  phase (the pixel-parity claim above).

### Phase M: additive multi-layer corona (glowstick redesign, follow-up to Phase K/L)

Full plan: `~/.claude/plans/yes-i-really-like-composed-hearth.md`. Phase K/L's `brightness` knob
(desaturate-toward-white `hot_color`, alpha-blended at one spatial scale) tested as "a lighter and
more desaturated color", not a glowstick — real bloom is additive (light from multiple scales adds
onto the background, clamping to white only where the sum saturates). Replaced entirely (no
back-compat shim, pre-1.0 format) with `light = color * Σ(layer[i].amplitude *
exp(-d/layer[i].sigma_px)) * brightness`, additive-blended (`ONE`/`ONE`), for barrier glow, note
glow, and particles/flashes. `GlowLayer { amplitude, sigma_px }` (3-tuple `layers` field, default
tight/mid/wide `[(2.6, 5px), (1.1, 16px), (0.38, 48px)]`) replaces `Glow::radius_px` everywhere;
`FlashSpec`/`ParticleSpec` each gained their own `layers` (reusing `GlowLayer`, additive schema
change). `BarrierLayer` gained an independent `show_bar: bool` (default `true`) — whether the flat
opaque bar renders at all, separate from `glow`.

Barrier and notes each need **two render pipelines** (glow pass, additive, drawn first; core pass,
alpha-blended, drawn second so it occludes the glow directly beneath it) — an opaque core and an
additive halo can't share one `wgpu::BlendState`. Particles/flashes didn't need the split (nothing
stacks on top of them the way video/notes do under barrier/notes, so "hard dot + additive halo"
can share one draw). All 8 shipped `examples/styles/*.fmstyle.ron` and
`crates/project/examples/dump_sample_styles.rs` migrated off `radius_px` to `layers`, sigmas seeded
by scaling the default layer set proportionally to each sample's old radius (not load-bearing,
starting points for hand-tuning). Note: RON serializes a fixed-size `[T; 3]` array with **tuple
parens `(...)`, not brackets `[...]`** — confirmed empirically (`ron`'s parser rejects
`layers: [...]`, only `layers: (...)` round-trips), easy to get wrong hand-editing a sample file.
`GlowLayer` needed adding to `project`'s crate-root re-export list (`crates/project/src/lib.rs`) —
missed in the initial schema pass, only surfaced once something outside `style.rs` needed to
construct one.

**Three bugs found after this landed, from actually looking at the result (not caught by
`cargo build`/`clippy`/tests, since none of them are type errors):**

1. **Note glow washed out the entire note interior, not just its edge.** `notes/shader.wgsl`'s
   `fs_core` applied `hot_color(fill_color, brightness)` unconditionally to every interior pixel
   once glow was enabled, not scoped by distance from the edge at all — a `brightness > 1` style
   (typical; `gradient-glow.fmstyle.ron` uses `2.0`) made the *whole note* a flat near-white blob,
   not a colored note with a hot rim.
2. **That same bug read as an unwanted hard "border" ring.** Because the whitened interior met the
   unwhitened, differently-colored corona (`fs_glow`, drawn separately) at a single ~1px
   antialiasing seam (`dist`'s `smoothstep(radius-0.5, radius+0.5, d)` band), the transition was
   abrupt rather than smooth — read as a distinct outline stuck to each note's edge, easily
   mistaken for the separate (still schema-only, unimplemented) `Border` field kicking in even when
   `border: None`. Same root cause as (1), one fix: `fs_core` now computes `inward_dist = max(radius
   - d, 0.0)` (distance from the note's true edge, exact within the `radius`-px band `dist`
   actually resolves, clamped — not extrapolated — beyond it, per that function's existing
   doc-commented limitation) and mixes toward `hot_color` with weight `exp(-inward_dist /
   glow_layers_ab.y)` (the corona's own tightest-layer sigma) instead of applying it flat. Deep
   interior pixels (more than a few px from the edge) now keep their true fill color unchanged at
   any brightness; the hot rim's peak (weight 1 at the true edge) hands off continuously into
   `fs_glow`'s corona (also strength-1 at `d_past_edge = 0`), so there's no seam anymore.
3. **`show_bar: false` didn't actually hide anything.** The opaque core pipeline *was* correctly
   skipped (`barrier.rs::render` only draws `core_pipeline` when `self.show_bar`), but
   `barrier.wgsl`'s `fs_glow` still zeroed its own additive output under the bar's full thickness
   footprint via `(1.0 - shape.core_alpha)` — `edge_shape`'s `edge_dist` saturates at 0 for the
   *entire* bar interior, not just its literal edge (same "unsigned distance, 0 well inside"
   convention `notes/shader.wgsl`'s `dist` uses), so that occlusion term was zeroing the corona
   across the whole bar-width band regardless of whether the (now-invisible) opaque core was
   actually there to justify it. Net effect: `show_bar: false` swapped "a flat colored bar" for "an
   equally visible flat gap showing whatever's behind it" — never actually producing a single solid
   glowing blade. Fixed by threading `show_bar` into the shader too (repurposed
   `glow_layers_c.w`, previously unused) and only applying the occlusion when it's `1.0`; with
   `show_bar: false` the corona now shines at full (edge) strength straight across the bar's
   footprint, reading as one continuous glowing column with no seam — the actual "hide the ugly
   flat patch, keep only the light" look `show_bar` was added for.

**Also this session**: a refresh button (⟳) next to "Import style…" in the Project tab
(`app/src/ui.rs`/`app/src/main.rs`) — `UiState` gained `style_path: Option<PathBuf>` (mirrored
whenever `load_style` succeeds, cleared on New Project / loading a `.fmproj.ron` project, since an
embedded project style has no external file to reload from) and `reload_style_requested: bool`,
so a `.fmstyle.ron` can be hand-edited externally and reloaded without reopening the file picker
each time. The button is disabled until a style has actually been imported from a file.

**Later addition**: a text field underneath "Import style…"/the refresh button, mirroring the
Project-file section's own path-text-field pattern, so a style can be loaded either via the native
file picker or by typing/pasting a path directly. `UiState` gained `style_path_text: String`
(mirrored from `style_path` — via `load_style` — every time a style is (re)imported by any means:
the picker, the refresh button, a CLI-passed style path, or a loaded project that references one;
cleared alongside `style_path` on New Project / loading a project, same reasoning as that field)
and `load_style_path_requested: bool` (set by a "Load" button next to the text field, or pressing
Enter in it; `main.rs`'s `apply_post_ui_updates` handles it identically to
`reload_style_requested` except the path comes from the typed text instead of `style_path`, and
it's a no-op on an empty field rather than needing an `Option` check). Shares `load_style` end to
end with every other style-loading path, so it gets the same status-message/error reporting as the
picker.

**Bug found on first real use**: pressing Enter to submit the path reliably produced a "path not
found" error, because the field's text actually gained a trailing space from that same Enter
keypress — egui's singleline `TextEdit` receives Enter both as a key event (which the "submitted"
check above reacts to) and as an insertable text event, and singleline mode maps a newline
character to a literal space rather than dropping it, so `style_path_text` ended up as e.g.
`"foo.fmstyle.ron "` by the time the submit check fired. Fixed by trimming the text before
constructing a `PathBuf` from it at every load-on-submit call site — not just this one:
`load_style`'s own `load_style_path_requested` handler, `save_project`/`load_project` (the
Project-file text field this one was modeled on has the identical latent gotcha even without
Enter-to-submit wiring, if a user presses Enter in it out of habit), and `start_export`'s
`export_path_text`. The picker-driven paths (`import_style_requested`, `open_project_requested`,
etc.) were never affected — `rfd::FileDialog` results don't round-trip through a text buffer.

**`show_bar` now defaults to `false`** (`#[serde(default)]` on the field, `BarrierLayer::default()`
follows suit) — once the (3) fix above made `show_bar: false` actually produce a clean single-blade
glow with no gap, the flat opaque bar stopped being a look worth defaulting to at all; a
`.fmstyle.ron` predating this field, or one that just never set it, now gets pure corona with no
bar unless it opts in with `show_bar: true`. Doesn't affect the no-imported-style (legacy slider)
look at all — `Style::from_legacy` builds its `BarrierLayer` with an explicit `show_bar: true`
literal, not the `Default` impl or serde's field default, so a project with no imported style still
shows its plain bar exactly as before. All 8 shipped sample styles already set `show_bar` (mostly
`true`) explicitly per Phase M's own sample migration above, so none of them changed look from this.

**Not yet manually run** (per the "never run the app yourself" rule): re-import
`barrier-pulse.fmstyle.ron` and `gradient-glow.fmstyle.ron` and confirm the glow now reads as a
genuine white-hot core radiating outward rather than a flat lighter color, with note interiors
keeping their true color; confirm `show_bar: false` on a glowing barrier now renders a single solid
glowing column with no visible gap or seam down the middle; confirm `sparks.fmstyle.ron`'s
particles/flash still look right; a rough performance glance during scrubbing (barrier/notes now
issue 2 draw calls instead of 1).

**Closed-out gaps from the original Phase M plan** (`~/.claude/plans/yes-i-really-like-composed-hearth.md`
§6/§7 — the implementation itself was already complete, but two tests and the format-doc rewrite
the plan called for hadn't actually landed):

- **Tests** (`crates/project/src/style.rs`): added
  `glow_layers_array_with_explicit_values_round_trips` (non-default `[GlowLayer; 3]` values, plus
  an explicit assertion that the serialized RON uses tuple-paren `layers: (...)` syntax, not
  `layers: [...]` — the existing round-trip tests all happened to only ever exercise
  `default_glow_layers()`, which wouldn't have caught a serialization-shape regression) and
  `barrier_layer_show_bar_defaults_to_false_when_omitted` (the plan's draft named this test
  "defaults_to_true", written before the later "`show_bar` now defaults to `false`" decision
  earlier in this doc — the test asserts the *current* `false` default, not the plan's original
  wording).
- **`docs/fmstyle-format.md`**: this file had never actually been updated for Phase M at all (still
  documented `Glow { color, radius_px, brightness }` and the Phase L reach-scaling mechanism as
  current) despite the plan explicitly requiring it. Rewrote the `Glow` section and RON examples
  around `layers: [GlowLayer; 3]`, added a `GlowLayer` reference under the section, added `layers`
  rows to the `ParticleSpec`/`FlashSpec` tables and a `show_bar` row to `BarrierLayer`'s, rewrote
  "Brightness/overexposure" with Phase M as the current mechanism (Phase L relabeled superseded,
  same convention already used for Phase K there), and added the Phase M row to the
  breaking-change log.
- **Found along the way, left alone**: `examples/styles/barrier-wavy-volume.fmstyle.ron` currently
  has an uncommitted local edit that breaks RON parsing — `glow: Some(Edge(` (an extra, invalid
  `Edge(...)` wrapper around the `Glow` literal, apparently a stray edit from hand-tuning the wavy
  barrier look) — which fails `shipped_sample_styles_parse`. This predates and is unrelated to the
  Phase M documentation/test work in this section; confirmed by stashing the working-tree diff and
  re-running the test, which passes on the last-committed version of that file. Not fixed here
  since it looked like in-progress hand-editing rather than a regression this session caused —
  worth a look before the next commit.

### Wavy barrier redesign: noise-based ripples instead of traveling sines (follow-up to the wavy work above)

The original `wavy_offset(x)` (introduced earlier in this doc, "three incommensurate-frequency
sine terms") combined a spatial term (`x * k`) and a temporal term (`t * speed`) *additively inside
the same phase* (`p1 = x*k + t*speed`, etc.) — which makes the ripple pattern a rigid shape
scrolling horizontally at a constant rate, and however many sine terms are summed, the result is
still exactly periodic in both x and t. Feedback from actually looking at it next to SeeMusic: it
reads as "a wave translating across the screen," not the irregular, occasionally-larger ripple
texture SeeMusic has, and there's no reason for the barrier's ripple to have net horizontal motion
at all.

Rewrote `wavy_offset` in `crates/render/src/barrier.wgsl` around a hand-rolled 2D value-noise
(`hash21`/`noise2`, hash-based, no textures) sampled on two *independent* axes — `x / wavelength_px`
for space and `t * speed` for time — rather than one combined traveling-phase term. This keeps the
existing three fields' meaning close to their old one but reinterpreted: `wavelength_px` is the
spatial scale of the noise (bigger = broader/calmer ripples), `speed` is how fast the noise field
mutates over time (no horizontal scrolling at any speed value), and `amplitude_px` is still the
baseline ripple size. Three noise octaves at incommensurate scales/rates (weights 0.55/0.30/0.15,
summing to 1.0, matching the old sine sum's weighting convention) keep the base field non-periodic
and bounded to `|n| <= 1`.

**"Occasionally you see a bigger one"**: a separate, much-lower-frequency noise sample
(`envelope_n`, scaled by `0.23`/`0.31` relative to the base octaves) drives a `1 +
(WAVE_ENVELOPE_MAX - 1) * envelope_n^2` envelope that multiplies the base ripple — squaring makes
the swell rare and its sign irrelevant (both large-positive and large-negative `envelope_n` produce
a swell), rather than a uniform-looking wobble. `WAVE_ENVELOPE_MAX = 2.6` is a real not just
theoretical bound (`n` bounded to `[-1,1]`, `envelope` bounded to `[1, 2.6]`), so the vertex
shader's rasterized-quad inflation margin (`wavy_margin` in `vs_main`) had to change from
`wave.x` to `wave.x * WAVE_ENVELOPE_MAX` to keep covering the true worst case — under-sizing that
margin would clip the biggest ripples against the quad's raster bounds instead of just not drawing
them, a much worse artifact than a slightly larger (mostly-empty) inflated quad.

**Not yet manually run**: re-import a style with a `wavy` barrier (e.g.
`examples/styles/barrier-wavy.fmstyle.ron`) and confirm the ripple now looks irregular/stochastic
rather than a scrolling wave, that it has no net horizontal drift at any `speed` value, that
occasional larger swells are visible without ever clipping against the bar's rasterized bounds, and
that `wavelength_px`/`amplitude_px` still read as "how calm are the ripples" as `speed` still reads
as "how fast do they fluctuate."

**Follow-up: flatten small deviations, amplify big ones.** The first cut above still looked too
uniformly wobbly — a straight sum of three signed noise octaves rarely lands near zero, so ordinary
moments already had noticeable size, before the envelope even multiplies anything up. Added a
power-law shaping step, `n = sign(n) * pow(abs(n), NOISE_SHAPE_POWER)` with `NOISE_SHAPE_POWER =
2.2`, applied to the octave sum right before the envelope multiply. Since `n` is bounded to `[-1,
1]`, raising `|n|` to a power > 1 pulls small values toward 0 much faster than it pulls values
already near +-1 (both endpoints and 0 are fixed points of any power), so the baseline reads calmer
while genuinely large coincidences of the octaves still reach close to full size — then the
existing envelope layers "occasionally much bigger" on top of that already-shaped field, instead of
on top of the uniformly-sized raw sum. Bound is unchanged (`|n_shaped| <= 1`, same as raw `n`), so
`WAVE_ENVELOPE_MAX` and the vertex shader's inflation margin didn't need to change.

### Phase N: canvas background color (both `.fmstyle.ron` and the legacy Keyboard-tab sliders)

Every render pass clearing the canvas (`app`'s interactive `preview_pass`, `export`'s
`export_scene_pass`) hardcoded `wgpu::Color::BLACK` — there was no way to change what shows through
behind the video (e.g. letterbox gaps from a `VideoTransform` crop/scale) or behind the note
highway above the barrier. Added a `background` option on both axes this schema already supports
per layer (imported `.fmstyle.ron` vs. the legacy "quick control" sliders), following the exact
pattern `effective_note_layer`/`effective_barrier_layer`/`effective_transition_layer` already
established:

- **Schema** (`crates/project/src/style.rs`): `Style` gained `background: ColorBinding`
  (`#[serde(default = "default_background_color")]`, black — matches the old hardcoded clear color
  so existing files render unchanged). Deliberately **not** wrapped in `Timed<T>` like `notes`/
  `barrier`/`transition` — those are per-layer looks that could plausibly change over a song
  (hence the time-keying spine), but a canvas background is one global value with no per-note
  timeline to key against; adding a `Timed<BackgroundLayer>` wrapper for a single color would be
  ceremony with no payoff. `Style::from_legacy` gained a third parameter, `background_color: [u8;
  3]` — background isn't note- or barrier-specific, so it doesn't belong on `NoteStyle`/
  `BarrierStyle`, and this mirrors how `from_legacy` already takes the other two structs by
  reference rather than bundling everything into one param.
- **Legacy path** (`crates/project/src/lib.rs`): `Project` gained `background_color: [u8; 3]`
  (`#[serde(default)]`, which for a `[u8; 3]` is `[0, 0, 0]` — black, no custom default fn needed,
  unlike the `ColorBinding` field which does need one since `ColorBinding` has no `Default` impl
  that happens to be black). `Project::effective_background_color()` added alongside the three
  existing `effective_*` methods, same "imported style wins, otherwise synthesize via
  `Style::from_legacy`" rule — the only difference from its siblings is there's no `Timed<T>` to
  `.resolve(0.0)` first, since `background` is a plain `ColorBinding`; it goes straight to
  `.resolve_constant()`.
- **`app`**: `UiState` gained `background_color: [u8; 3]`, mirrored to/from `Project` at every site
  `barrier_style`/`note_style` already are (new-project reset, save snapshot, load-project). A new
  free function `effective_background_color(ui_state: &UiState)` mirrors the existing
  `effective_note_layer`/`effective_barrier_layer`/`effective_transition_layer` free functions in
  `main.rs` (same reason those exist as free functions instead of `Project` methods: `UiState` isn't
  a `Project`, no paths to snapshot just to resolve one color). The Keyboard tab (`draw_keyboard_tab`
  in `app/src/ui.rs`) gained a "Background" section (color picker + reset button) after the existing
  Barrier/Note style sections — same tab, since that's already where the legacy quick-control
  sliders for barrier/note visuals live, not a new tab.
- **Wiring the clear color**: both `app/src/main.rs`'s `preview_pass` and `crates/export/src/lib.rs`'s
  `export_scene_pass` now clear to `effective_background_color(...)`/`project.
  effective_background_color()` instead of `wgpu::Color::BLACK`. Both render targets are meant to
  hold **linear**-space values before any final display/encode step (every other color in this
  codebase — note fill, barrier glow, particle color — goes through the same `srgb_to_linear`
  formula before being written into a uniform or vertex attribute), so the resolved `[u8; 3]` is
  converted through a **fourth, independent copy** of the identical `srgb_to_linear` formula
  (`crates/render/src/barrier.rs`/`crates/render/src/effects.rs` each already have their own private
  copy; `app`/`export` needed their own too, `render`'s copies are private to that crate) before
  being passed as the `wgpu::Color` clear value — clearing to a raw sRGB byte reinterpreted as linear
  would produce a visibly different (darker, for any non-gray color) shade than the same color drawn
  as a fill elsewhere.
- **Sample style**: left the 8 existing shipped `examples/styles/*.fmstyle.ron` files on the black
  default (this is a canvas-level option orthogonal to what each of those is actually showcasing —
  glow, wavy, particles, etc. — not worth perturbing 8 existing "known good" reference files for)
  and added a 9th, new `examples/styles/dark-background.fmstyle.ron`, generated the same way every
  other sample is (a `Style` literal in `crates/project/examples/dump_sample_styles.rs`, run and
  copied into the checked-in file — not hand-typed, so it's guaranteed to match this `ron` version's
  actual output). Deliberately minimal outside of `background` itself: a solid-fill note color and a
  modest barrier glow (`show_bar: false`, so it doesn't just read as "a plain white line on a dark
  background") are the only other non-default fields, so the dark-navy `background: Constant((8, 10,
  24))` canvas color is unambiguously what the sample demonstrates.

**Not yet manually run**: confirm the Keyboard tab's new "Background" color picker actually changes
the preview's canvas color; confirm a project saved with a non-black background reloads with the
same color; confirm an exported clip's background matches the preview (same `srgb_to_linear`
formula, so it should, but worth a real ffprobe/frame-extraction check); confirm importing a
`.fmstyle.ron` with a `background` field overrides the legacy picker as expected, and that removing
the import falls back to the legacy color again, not black.

### Phase O: barrier strand bundle, ported from `explorations/barrier-fx-lab`

`explorations/barrier-fx-lab/barrier-fx-lab.html` (a standalone WebGL2 playground, not built/wired
into the app — see its own `README.md`) had prototyped several barrier looks beyond what
`barrier.wgsl` actually implemented: a "strand bundle" (several thin, independently-flickering
filament threads fraying off the wavy top edge, rather than one smooth wavy line — the closest
match found so far to the real SeeMusic edge, see `sm-ex.png`), plus a separate "sliding electric
filament" and "wisps" experiment. This phase ports **only the strand bundle** into the real
renderer/schema; the filament/wisp controls remain lab-only, unported.

- **Schema** (`crates/project/src/style.rs`): new `StrandSpec` struct (`count`, `spread_px`,
  `jitter`, `thickness_px`, `halo_amplitude`, `halo_sigma_px`, `glow_intensity`, `flicker_speed`,
  all `#[serde(default = ...)]` so existing `.fmstyle.ron` files without the field keep parsing
  unchanged) and a new `strands: Option<StrandSpec>` field on `WavySpec`. Defaults are the lab's
  own generic `SCHEMA` defaults (count 5, spread 14px, jitter 0.75, thickness 1.4px, halo amp
  1.0/sigma 6px, glow intensity 1.3, flicker speed 1.8), not any one preset's tuned values.
  Deliberately **not** its own color: strands render inside the corona (`fs_glow`) pass and are
  tinted by the barrier's own `Glow::color`/brightness, so `glow: Some(..)` is required for
  strands to actually be visible — `strands: Some(..)` with `glow: None` parses/round-trips but
  renders nothing. Re-exported from `crates/project/src/lib.rs`.
- **Renderer** (`crates/render/src/barrier.wgsl` + `barrier.rs`): `Uniforms` gained two more vec4s
  (`strand_params_a`/`strand_params_b`, packing count/spread/jitter/thickness and halo-amplitude/
  halo-sigma/glow-intensity/flicker-speed) uploaded unconditionally by `BarrierRenderer::set_style`
  whenever `WavySpec::strands` is `Some(..)`, regardless of `mode` — there is **no CPU-side gate**
  on `WavyMode::Edge`. `wavy_offset` was generalized to `wavy_offset_seeded(x, seed)` (a `seed`
  parameter shifts which region of the noise field gets sampled, decorrelating one strand's ripple
  from another's; `seed == 0.0` reproduces the original single-edge behavior exactly, which is all
  `wavy_offset` itself — kept as a thin wrapper — ever needs), and `EdgeShape` gained `top_edge`/
  `base_wave` fields so `fs_glow`'s new strand loop can compute each strand's own offset relative
  to the edge `fs_core`/the base corona already use. **`fs_glow` is the single place that actually
  gates strand rendering to `Edge` mode** (`flags.w` in `(0.5, 1.5)`) — this was a deliberate
  choice to avoid encoding the "only in Edge mode" restriction in two places (Rust and WGSL) that
  could drift; ported line-for-line from the lab's own `edgeMode`/strand-loop logic, adjusted only
  for wgsl's y-down vs. the lab's WebGL y-up fragment-coordinate convention (see the wgsl file's
  own comments on `edge_y_i`'s sign). `vs_main`'s quad-inflation margin also grew a
  `strand_margin` term (`spread_px + halo_sigma_px * 5`, same `exp(-d/sigma)` 8-bit-invisibility
  cutoff `barrier.rs`'s `GLOW_CUTOFF_SIGMAS` already uses for the ordinary corona layers) gated on
  the same `glow enabled && edge mode && count >= 1` condition, so strand halos actually have
  pixels to paint onto without inflating the quad for styles that don't use strands.
- **Sample style**: left the existing `barrier-wavy`/`barrier-wavy-volume`/`showcase_blue_purple`
  samples alone (`strands: None`, unchanged look) and added a new, dedicated
  `examples/styles/barrier-strands.fmstyle.ron` (generated the same way every other sample is, via
  `crates/project/examples/dump_sample_styles.rs`) — values close to the lab's own "SeeMusic gold"
  preset (`explorations/barrier-fx-lab/presets/seemusic-found.json` is a *different*, more extreme
  hand-tuned preset also using strands, not this one).
- **No UI editor.** Like every other `.fmstyle.ron` field, there's no slider for this in `app/src/
  ui.rs` — styles are edited as RON text and imported via the Project tab's "Import style…" button,
  same as every field documented in `docs/fmstyle-format.md`.

**Not yet manually run**: load `barrier-strands.fmstyle.ron` and confirm the strand bundle actually
renders and reads as several independent flickering threads (not, say, one thick blurred band);
confirm strands disappear when switching a style's `mode` to `TopWave`/`FullWave` without touching
`strands` itself; confirm a style with `strands: Some(..)` but `glow: None` renders with no visible
strands (silent no-op, not a crash); confirm the pulse brightness and wavy ripple still read
correctly with strands layered on top, at both low and high `strandCount`/`jitter` (up to the
8-strand cap).

### Phase O follow-ups: exact preset match, hand-tuning, `slide_speed`, and a refresh script

Three smaller changes landed after Phase O's initial PR, all still within the strand-bundle scope:

- **`examples/styles/barrier-strands.fmstyle.ron` first matched `seemusic-found.json` exactly**
  (`layers[i].amplitude` = `layerAmpN * glowIntensity` since this schema bakes the lab's separate
  intensity knob into amplitude directly, `pulse` initially carried over as `brightnessPeak`/
  `pulseDecay`), **then had `pulse` removed** (`pulse: None`, so the barrier holds at a steady
  resting glow instead of brightening on each note) **and several values hand-tuned further**
  (`thickness: 0.0`, `strands.spread_px: 4.0`, `strands.thickness_px: 2.0`,
  `strands.glow_intensity: 0.5` — all diverging from a literal preset translation by eye/taste, not
  by a translation rule). `crates/project/examples/dump_sample_styles.rs`'s `barrier_strands` block
  is the source of truth for whatever this sample's current values actually are; its own doc
  comment documents the translation rules, not this milestone doc.
- **`WavySpec` gained `slide_speed: f32`** (`#[serde(default)]`, `0.0` = exact no-op), porting the
  lab's `slideSpeed` parameter: unlike `speed` (which mutates the ripple's *shape* in place over
  time, leaving its x-position fixed), `slide_speed` literally translates the noise field sideways
  along x — a positive value reads as "current flowing through the wire," not just wobbling in
  place. Implementation: `barrier.wgsl`'s `wavy_offset_seeded` now computes `x_slid = x - t *
  slide_speed` before dividing by `wavelength` (both `sx` axes downstream use `x_slid`, `st` is
  unaffected). The uniform value is parked in `glow_style`'s previously-unused `w` component
  (`crates/render/src/barrier.rs`) rather than growing the `Uniforms` struct by another vec4, same
  "reuse a documented spare slot" pattern `bar_color.w` already established. **Affects the whole
  wavy edge, every `WavyMode`, not just the strand bundle** — strands re-sample the same
  `wavy_offset_seeded` field the base edge itself uses, so they slide in lockstep with it
  automatically, with no strand-specific slide field needed. `barrier-strands.fmstyle.ron` sets
  `slide_speed: 40.0` (the `seemusic-found.json` value); the other three `WavySpec`-using samples
  (`barrier-wavy`, `barrier-wavy-volume`, `showcase_blue_purple`) got `slide_speed: 0.0` (no-op,
  unchanged look).
- **`scripts/refresh-sample-styles.sh`** automates the "copy stdout sections into files" step
  `dump_sample_styles.rs`'s own header comment describes: runs the example, splits its `=== name
  ===`-delimited stdout, and writes each block **raw** (no reformatting) into
  `examples/styles/<name>.fmstyle.ron`. Content-equivalent to hand-copying, but the raw dump's
  formatting differs cosmetically from what most of the 10 shipped files previously had by hand —
  expanded `layers: ((
 amplitude: ...,
), ...)` tuples instead of a compact one-line-per-entry
  form, and explicit default-valued fields (`edge_blend_px: 0.0`, `background: Constant((0, 0,
  0))`) that used to be stripped by hand. All 10 files were refreshed to this raw form in this
  session — same underlying `Style` values, different formatting only (verified via `cargo test -p
  project`'s `shipped_sample_styles_parse`, which re-parses every file).

### Phase P: canvas-Y-position note gradient (`Fill::CanvasGradient`)

The first roadmap item off the README's "`.fmstyle.ron`" list: "Y-level-dependent note styles" —
notes whose color depends on where they currently are on the canvas, not on which key/pitch they
belong to, so every note passing through the same on-screen height reads the same color there. The
only design that made sense for a first cut is a 2-color gradient, same shape as the existing
per-note `Fill::VerticalGradient`, just blended across a different span.

- **Schema** (`crates/project/src/style.rs`): `Fill` gained a third variant, `CanvasGradient { top:
  ColorBinding, bottom: ColorBinding }` — identical shape to `VerticalGradient`, different meaning.
  `VerticalGradient` blends across each note's own on-screen height (every note shows the full
  `top`->`bottom` range wherever it is); `CanvasGradient` blends across a fixed span of the canvas
  itself instead (canvas Y = 0 at the top of the frame down to the barrier line resolves `top`-
  >`bottom`, clamped beyond either end) — a falling note's rendered color changes over time as it
  descends through that span, rather than carrying a fixed range baked in. The two variants are
  mutually exclusive by construction (one `Fill` enum, one variant per fill) — no separate
  compatibility flag needed for that. `black_key_fill` (`BlackKeyFill::Auto`/`Same`/`Custom`) works
  identically to every other fill variant: `Auto` darkens the resolved endpoints for sharp keys
  (still canvas-blended), `Custom(fill)` can independently choose a *different* `Fill` variant for
  sharp keys than natural keys use (including `CanvasGradient` on one side and `Solid`/
  `VerticalGradient` on the other) — this "just works" with no extra schema plumbing since
  `BlackKeyFill` already resolves the two key groups' fills completely independently. `sheen`/
  `glow` are unaffected either way — both are computed independently of `Fill` in the fragment
  shader, so `CanvasGradient` composes with either exactly as `Solid`/`VerticalGradient` already do.
- **Renderer** (`crates/render/src/notes/`): `resolve_fill_base` (`mod.rs`) already resolved
  `VerticalGradient`'s `top`/`bottom` `ColorBinding`s to a concrete color pair per rebuild;
  `CanvasGradient` reuses the exact same resolution (added to the same match arm, since resolving
  *which* colors doesn't depend on how they'll be blended). What differs is *how* the shader mixes
  those two colors, which can't be inferred from the color values alone (unlike the existing
  solid-vs-gradient distinction, which the shader gets for free since a solid fill's two colors are
  simply equal) — so a new per-note flag was added: `NoteInstance::canvas_gradient: f32` (`0.0`/
  `1.0`), a new vertex attribute (location 8) alongside the existing `velocity`/`track_index`
  fields added ahead of need for the same future-property-binding phase. `rebuild_instances`
  computes it once per key group per rebuild (`is_canvas_light`/`is_canvas_dark`, mirroring how
  `top_light`/`top_dark` etc. are already computed once per group) via a small
  `is_canvas_gradient(fill: &Fill) -> bool` helper, then bakes the right one into each note
  instance depending on `key.is_sharp` — same pattern the existing color-pair selection already
  used, just one more parallel field. This needed to be per-instance rather than a style-wide
  uniform flag for the same reason the color pair itself is per-instance: `BlackKeyFill::Custom`
  can make the two key groups differ.
- **Shader** (`shader.wgsl`): `NoteInstance`/`VertexOutput` both gained the `canvas_gradient` field
  (location 8 on the instance, location 5 on the output — passed through unmodified in `vs_main`).
  `fill_color` now computes two candidate UV fractions — `note_uv_y` (the original note-local
  fraction, unchanged) and `canvas_uv_y` (`in.position.y / (view_uniform.size.y *
  view_uniform.barrier_fraction)`, using the fragment's absolute framebuffer position instead of
  its position within the note, normalized against the same `keyboard_y` the vertex shader already
  computes) — and `select()`s between them based on the per-fragment `canvas_gradient` flag before
  mixing `color_top`/`color_bottom`. Everything downstream of that mix (sheen, the glow rim
  blend-target in `fs_core`, the corona in `fs_glow`) is unchanged, since none of it reads `Fill`
  or the gradient basis at all.
- **Bug found on first real run**: reading `view_uniform` from `fill_color` (called by both
  fragment entry points) crashed at pipeline creation — `wgpu` validation error, "Shader global
  ResourceBinding { group: 0, binding: 0 } is not available in the pipeline layout / Visibility
  flags don't include the shader stage." Group 0's bind group layout (`pipeline.rs`'s
  `view_bind_group_layout`) only granted `ShaderStages::VERTEX`, left over from when
  `view_uniform` was vertex-only (the transform/scale/barrier_fraction it carries were only ever
  read in `vs_main` before this phase). Fixed by adding `ShaderStages::FRAGMENT` to that entry's
  `visibility` — the uniform itself didn't need to change, only which stages are allowed to bind
  it. Cargo build/clippy don't catch this class of bug (same gotcha `docs/fmstyle-history.md`'s
  "Black-key gradient bug" section already notes for embedded WGSL: validation only happens when
  the pipeline/shader module is actually created at runtime) — worth double-checking bind group
  layout `visibility` flags whenever a shader change starts reading an existing uniform from a
  new stage.
- **Sample style**: `examples/styles/canvas-gradient.fmstyle.ron`, generated the same way every
  other sample is (`crates/project/examples/dump_sample_styles.rs`'s `canvas_gradient` block +
  `scripts/refresh-sample-styles.sh`) — deep blue near the top of the frame fading to warm gold
  near the barrier, with a modest sheen, and `black_key_fill` set to an independent (dimmer)
  `CanvasGradient` rather than `Auto`, so the sample also demonstrates that path.
- **No UI editor**, same as every other `.fmstyle.ron` field not already surfaced by the Keyboard
  tab's legacy sliders — edited as RON text, imported via the Project tab's "Import style…" button.

**Not yet manually run**: load `canvas-gradient.fmstyle.ron` and confirm notes actually shift color
as they fall (blue near the top of the highway, gold approaching the barrier) rather than each note
carrying one fixed color or a fixed per-note gradient; confirm sharp keys render their own dimmer
`CanvasGradient` correctly; confirm sheen still visibly sweeps across notes using this fill;
confirm switching back to a `VerticalGradient`/`Solid` style still looks exactly as before (the new
per-instance flag defaults to `0.0`/note-local for every other fill).

### Phase Q: flashes/particles/glow that match note color, and multicolor flashes

Off the README's roadmap: "Flashes/particles that match note color." Full field-by-field spec is
in `docs/fmstyle-format.md` (`Glow.match_note_color`, `ParticleColor`, `FlashColor`, and the
"Note-bottom color sampling" section) — this is the narrative of what was built and why.

Three genuinely ambiguous design questions were asked before implementing (all answered before any
code was written):
1. Whether particles' "match note color" and "y-level gradient" should be independent toggles or
   one mutually-exclusive mode selector — **answered: single enum** (`ParticleColor`).
2. Whether flashes should get a general-purpose author-painted gradient in addition to the
   auto-derived note-matching look — **answered: both** (`FlashColor::HorizontalGradient` alongside
   `FlashColor::MatchNoteBottom`).
3. Whether `ParticleSpec.color`/`FlashSpec.color` could be a clean breaking rename (from
   `ColorBinding` to a new enum) rather than a parallel field — **answered: yes**, same precedent as
   Phase H's `FlashSpec.radius_px` rename.

**Schema** (`crates/project/src/style.rs`):
- `Glow` gained `match_note_color: bool` (`#[serde(default)]`, `false`). Only meaningful for
  `NoteLayer::glow` — documented no-op for `BarrierLayer::glow`, same precedent as `edge_blend_px`.
- New `ParticleColor` enum (`Fixed(ColorBinding)` / `MatchNoteBottom` / `YGradient { top, bottom }`)
  replaces `ParticleSpec.color: ColorBinding`.
- New `FlashColor` enum (`Solid(ColorBinding)` / `HorizontalGradient(Vec<ColorBinding>)` /
  `MatchNoteBottom`) replaces `FlashSpec.color: ColorBinding`.
- Every shipped `examples/styles/*.fmstyle.ron` with a particle/flash `color:` field was hand-edited
  to the new enum shape (`Constant(...)` -> `Fixed(Constant(...))`/`Solid(Constant(...))`, targeted
  edits per this milestone's usual convention, not a wholesale regenerate); every `Glow` literal
  (in both the generator and the checked-in files) gained `match_note_color: false`. New sample
  `examples/styles/match-note-color.fmstyle.ron` demonstrates all three "match note" mechanisms
  together: a gradient+sheen note whose glow samples its own fill, plus a particle burst and a
  flash both colored from the note's own bottom color.

**`MatchNoteBottom`, for both `ParticleColor` and `FlashColor`, resolves to the triggering note's
own already-computed `color_bottom`** — a single flat color per note (`render::notes::
NoteInterval::color_bottom`, the exact value baked into that note's own `NoteInstance`), not a
finer per-pixel sample of its actual rendered fill/sheen. This is a deliberate simplicity/fidelity
tradeoff, not an oversight: every `Fill` variant, current or future, resolves to a `(top, bottom)`
pair by construction (`resolve_fill_base`'s contract), so this stays correct for any future
fill/sheen effect with zero render-side code to keep in sync — the alternative (sampling several
points across the note's rendered bottom edge to also catch things like a diagonal `Sheen` stripe)
was tried first and retracted, since it meant hand-porting the note shader's fill/sheen math into a
separate Rust function that would only ever track the one effect it was written against, silently
falling behind any other future note-color effect. `ParticleColor::MatchNoteBottom` is resolved
once per note per frame and reused for every particle that note spawns that frame (`effects.rs`'s
`resolve_particle_color`, called from `spawn_particles`/the continuous-emission loop);
`FlashColor::MatchNoteBottom` fills every one of a flash's `FLASH_GRADIENT_STOPS` (5) gradient
stops with that same one color (the same "uniform stops" trick a plain particle color already
uses), so it renders identically to `Solid` with that color — for a genuinely multicolor flash,
`HorizontalGradient` is the mechanism to reach for.

**Note glow's `match_note_color`, by contrast, doesn't have (or need) this tradeoff**: it calls
back into the note shader's own `fill_color_at` function (`crates/render/src/notes/shader.wgsl`,
refactored from `fill_color(in)` to `fill_color_at(in, at: vec2<f32>)` so it can be evaluated at an
arbitrary point, not just the current fragment) rather than a separate Rust port — a new
`note_edge_color(in)` clamps the fragment position into the note's true box (an approximation of
"nearest point on the silhouette" that ignores corner rounding, which only ever affects alpha, not
color) and evaluates `fill_color_at` there. `fs_core`'s rim-target and `fs_glow`'s corona color both
`select()` between `style_uniform.glow_color.rgb` and `note_edge_color(in)` based on a new
`StyleUniform.fill_and_flags.w` flag, so a note's halo automatically reflects sheen (and any future
fill-affecting shader change) with nothing to keep in sync — the particle/flash CPU simulation
just can't reach the fragment shader directly, so `color_bottom` is the pragmatic equivalent there.

**`FlashColor::HorizontalGradient`** (an author-painted, arbitrary-length gradient, independent of
note-matching) resamples onto `effects.rs`'s `FLASH_GRADIENT_STOPS`-stop (5) internal
representation at spawn time (`resample_gradient_stops`) — the same fixed stop count every
`EffectInstance` carries as `color_stops: [[f32; 3]; FLASH_GRADIENT_STOPS]` (replacing a single
`color: vec3`; a particle just has every stop baked equal, pixel-identical to a plain `color`
field). `wgpu::vertex_attr_array!` can't loop over a `const`, so the 5 stops are hand-unrolled to
explicit locations in both `EffectInstance::attributes()` and `effects.wgsl`, guarded by a
`const _: () = assert!(FLASH_GRADIENT_STOPS == 5, ...)` tripwire; `effects.wgsl`'s `fs_puff`/
`fs_glow` interpolate across the 5 stops by the fragment's horizontal position within `core_radius`
(not `quad_radius`, so the gradient doesn't distort into the glow margin).

**`ParticleColor::YGradient`** is the one mode where a particle's color isn't fixed at spawn:
`Particle` gained `y_gradient: Option<(top, bottom)>` plus `brightness`/`additive`, so `update`'s
per-step loop can recompute `color` from the particle's *current* canvas Y every frame — and reapply
the same post-processing `spawn_one_particle` applied once at spawn (additive particles' `color` is
never whitened, only `layer_amp` carries brightness, so the recompute is a plain `mix3`;
non-additive "puff" particles have `color` whitened via `hot_color` at spawn, so the recompute
reapplies `hot_color` too — the one correctness bug caught before shipping: an earlier draft
dropped this and silently lost `brightness`'s whitening on the very next frame after spawn).

**Verified**: `cargo build`, `scripts/check.sh` (fmt+clippy), and `cargo test --workspace` all
clean; `shipped_sample_styles_parse` finds the new `match-note-color.fmstyle.ron` alongside every
existing sample (all migrated to the new `color` enum shapes, with no schema/RON changes needed for
the `MatchNoteBottom` simplification itself). New unit tests in `crates/project/src/style.rs`
round-trip `Glow.match_note_color`, all three `ParticleColor` variants, and all three `FlashColor`
variants (including a multi-stop `HorizontalGradient`). **Not yet manually run** (per the "never
run the app yourself" rule) — worth confirming, next time someone has hands on the app: importing
`match-note-color.fmstyle.ron` shows the note's own halo shifting color top-to-bottom rather than
one flat glow color, and both the particle burst and the flash pick up the note's own bottom color;
a `YGradient` particle style visibly shifts a particle's color as it falls/rises rather than
staying fixed from spawn; a `HorizontalGradient` flash (multiple hand-picked stops) shows a genuine
left-to-right color sweep across the ellipse, not a flat color; and every pre-existing sample style
(`sparks`, `grinding-particles`, `key-glow`, `ellipse-flash`, `showcase_blue_purple`) still looks
pixel-identical to before this phase now that their `color` fields are `Fixed(...)`/`Solid(...)`.

