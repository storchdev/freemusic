//! `.fmstyle.ron` format: a data-driven description of note/barrier/transition visuals, designed
//! to be extended without breaking older files (every field is `#[serde(default)]`-compatible via
//! the wrapper types below). This module only defines the schema and its resolution helpers —
//! nothing here renders anything yet; see `CLAUDE.md` for which renderer phase consumes it.
//!
//! For the field-by-field contract (defaults, meaning, RON snippets, breaking-change log), see
//! `docs/fmstyle-format.md` — keep it in sync whenever this module's schema changes.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{BarrierStyle, NoteStyle};

fn current_style_version() -> u32 {
    1
}

/// Black, matching the hardcoded clear color both `app` and `export` used before this field
/// existed — an old/simple `.fmstyle.ron` (or the no-imported-style legacy path, see
/// `from_legacy`) gets the same canvas background as before, not an arbitrary default.
fn default_background_color() -> ColorBinding {
    ColorBinding::Constant([0, 0, 0])
}

/// Top-level `.fmstyle.ron` document: a resolved (or time-keyed) look for each of the three
/// visual axes this milestone proves out. `version` exists so a future breaking format change has
/// somewhere to branch on; unrecognized/missing fields fall back to defaults via `serde(default)`
/// on every field below, so older files stay loadable as the format grows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Style {
    #[serde(default = "current_style_version")]
    pub version: u32,
    #[serde(default)]
    pub notes: Timed<NoteLayer>,
    #[serde(default)]
    pub barrier: Timed<BarrierLayer>,
    #[serde(default)]
    pub transition: Timed<TransitionLayer>,
    /// Canvas clear color, visible behind the video wherever it doesn't fully cover the frame
    /// (e.g. a `VideoTransform` crop/scale leaving letterbox gaps) and behind the note highway
    /// above the barrier. Not per-layer/time-keyed (unlike `notes`/`barrier`/`transition`) since
    /// it's a single canvas-wide value, not something that varies per note or needs the `Timed`
    /// extensibility spine — a plain `ColorBinding` (only `Constant` actually renders, same as
    /// every other color field in this schema) is all a background needs.
    #[serde(default = "default_background_color")]
    pub background: ColorBinding,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            version: current_style_version(),
            notes: Timed::default(),
            barrier: Timed::default(),
            transition: Timed::default(),
            background: default_background_color(),
        }
    }
}

impl Style {
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::new())
            .map_err(|err| format!("failed to serialize style: {err}"))?;
        std::fs::write(path, text).map_err(|err| format!("failed to write {path:?}: {err}"))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("failed to read {path:?}: {err}"))?;
        ron::from_str(&text).map_err(|err| format!("failed to parse {path:?}: {err}"))
    }

    /// Produces the exact look the legacy `NoteStyle`/`BarrierStyle` sliders already draw —
    /// `Fill::Solid`, no sheen/glow, `BarrierKind::Line`, `TransitionKind::None` — so the renderer
    /// can always consume a `Style`, whether it was imported from a file or synthesized from
    /// whatever the Keyboard tab's sliders currently hold. `background_color` is the Keyboard
    /// tab's own background color picker (`Project::background_color`), not part of `NoteStyle`/
    /// `BarrierStyle` since it isn't note- or barrier-specific.
    pub fn from_legacy(
        note_style: &NoteStyle,
        barrier_style: &BarrierStyle,
        background_color: [u8; 3],
    ) -> Self {
        Self {
            version: current_style_version(),
            notes: Timed::Static(NoteLayer {
                fill: Fill::Solid(ColorBinding::Constant(note_style.color)),
                sheen: None,
                glow: None,
                roundedness: note_style.roundedness,
                fall_speed: note_style.fall_speed,
                border: None,
                black_key_fill: match note_style.black_key_color {
                    crate::BlackKeyColorMode::Auto => BlackKeyFill::Auto,
                    crate::BlackKeyColorMode::Same => BlackKeyFill::Same,
                    crate::BlackKeyColorMode::Custom(color) => {
                        BlackKeyFill::Custom(Fill::Solid(ColorBinding::Constant(color)))
                    }
                },
            }),
            barrier: Timed::Static(BarrierLayer {
                color: ColorBinding::Constant(barrier_style.color),
                thickness: barrier_style.thickness,
                glow: None,
                pulse: None,
                wavy: None,
                show_bar: true,
            }),
            transition: Timed::Static(TransitionLayer::default()),
            background: ColorBinding::Constant(background_color),
        }
    }
}

/// Generic "resolved now, or keyed over time" wrapper — the extensibility spine that lets any
/// layer be time-keyed later without a format break. `Keyed` is a sparse list of `(time_seconds,
/// value)` pairs; `resolve` picks the last key at or before `t`, clamped to the first key if `t`
/// precedes all of them. v1 only ever calls `resolve(0.0)` once at load time (no live mid-song
/// style swapping yet) — see the `// TODO` at each call site for where per-frame re-resolution
/// would hook in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Timed<T> {
    Static(T),
    Keyed(Vec<(f64, T)>),
}

impl<T> Timed<T> {
    pub fn resolve(&self, t: f64) -> &T {
        match self {
            Timed::Static(value) => value,
            Timed::Keyed(keys) => {
                debug_assert!(!keys.is_empty(), "Timed::Keyed must have at least one key");
                keys.iter()
                    .rev()
                    .find(|(key_t, _)| *key_t <= t)
                    .map(|(_, value)| value)
                    .unwrap_or(&keys[0].1)
            }
        }
    }
}

impl<T: Default> Default for Timed<T> {
    fn default() -> Self {
        Timed::Static(T::default())
    }
}

/// Warns exactly once per process that a note-property-driven binding was parsed but isn't wired
/// to real per-note data yet (`ByVelocity`/`ByPitchClass`/`ByTrack` — this milestone's documented
/// first extension point), then falls back to a representative constant colour/value.
fn warn_binding_not_yet_rendered_once() {
    use std::sync::Once;
    static WARNED: Once = Once::new();
    WARNED.call_once(|| {
        eprintln!(
            "style: a non-Constant binding was used, but property-driven bindings are \
             schema-only in this version — falling back to a representative constant"
        );
    });
}

/// A per-note color, either fixed or (schema-only this milestone) driven by a note property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ColorBinding {
    Constant([u8; 3]),
    ByVelocity(Ramp),
    ByPitchClass([[u8; 3]; 12]),
    ByTrack(Vec<[u8; 3]>),
}

impl ColorBinding {
    /// Resolves to a single representative color: exact for `Constant`, a documented fallback
    /// for the property-driven variants (`ByVelocity`'s high end, `ByPitchClass`'s first entry,
    /// `ByTrack`'s first entry or white if empty) until those are wired to real per-note data.
    pub fn resolve_constant(&self) -> [u8; 3] {
        match self {
            ColorBinding::Constant(color) => *color,
            ColorBinding::ByVelocity(ramp) => {
                warn_binding_not_yet_rendered_once();
                ramp.high
            }
            ColorBinding::ByPitchClass(colors) => {
                warn_binding_not_yet_rendered_once();
                colors[0]
            }
            ColorBinding::ByTrack(colors) => {
                warn_binding_not_yet_rendered_once();
                colors.first().copied().unwrap_or([255, 255, 255])
            }
        }
    }
}

impl Default for ColorBinding {
    fn default() -> Self {
        ColorBinding::Constant([255, 255, 255])
    }
}

/// Low/high color endpoints for a `ByVelocity` binding (velocity 0 -> `low`, 127 -> `high`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ramp {
    pub low: [u8; 3],
    pub high: [u8; 3],
}

/// A per-note scalar, either fixed or (schema-only this milestone) driven by a note property —
/// same shape and fallback rules as `ColorBinding`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScalarBinding {
    Constant(f32),
    ByVelocity { low: f32, high: f32 },
    ByPitchClass([f32; 12]),
    ByTrack(Vec<f32>),
}

impl ScalarBinding {
    pub fn resolve_constant(&self) -> f32 {
        match self {
            ScalarBinding::Constant(value) => *value,
            ScalarBinding::ByVelocity { high, .. } => {
                warn_binding_not_yet_rendered_once();
                *high
            }
            ScalarBinding::ByPitchClass(values) => {
                warn_binding_not_yet_rendered_once();
                values[0]
            }
            ScalarBinding::ByTrack(values) => {
                warn_binding_not_yet_rendered_once();
                values.first().copied().unwrap_or(1.0)
            }
        }
    }
}

impl Default for ScalarBinding {
    fn default() -> Self {
        ScalarBinding::Constant(1.0)
    }
}

/// Note fill: solid color, a per-note top-to-bottom gradient, or a canvas-Y-position gradient.
///
/// `VerticalGradient` and `CanvasGradient` both interpolate the same two `ColorBinding`s but over
/// different spans, and are mutually exclusive (a note picks exactly one `Fill` variant):
/// `VerticalGradient` blends across each note's own on-screen height, so every note shows the full
/// `top`->`bottom` range regardless of where it currently sits on the canvas — this is what makes
/// it a *per-note* gradient. `CanvasGradient` instead blends across a fixed span of the canvas
/// itself (canvas y = 0 at the top of the frame down to the barrier line), so whatever note is
/// passing through a given on-screen height shows the same color there regardless of pitch/key —
/// falling notes shift color as they descend rather than each carrying a fixed top-to-bottom
/// range. See `docs/fmstyle-format.md` for the exact mapping and render-side mechanics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Fill {
    Solid(ColorBinding),
    VerticalGradient {
        top: ColorBinding,
        bottom: ColorBinding,
    },
    CanvasGradient {
        top: ColorBinding,
        bottom: ColorBinding,
    },
}

impl Default for Fill {
    fn default() -> Self {
        Fill::Solid(ColorBinding::default())
    }
}

/// How black (sharp) keys' notes are colored relative to the white-key `fill`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum BlackKeyFill {
    /// Today's only behavior: darken the white-key fill by 0.6 — default, pixel-parity.
    #[default]
    Auto,
    /// No darkening, identical to the white-key fill.
    Same,
    /// Independently resolved fill (solid or gradient) for black keys.
    Custom(Fill),
}

/// Diagonal specular stripe swept across a note's fill.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sheen {
    pub intensity: f32,
    pub width: f32,
    pub angle_degrees: f32,
}

fn default_brightness() -> f32 {
    1.0
}

/// One exponential falloff term in an additive corona sum (Phase M): `amplitude *
/// exp(-d / sigma_px)`, where `d` is distance outside the glowing surface's opaque edge. A
/// `Glow`/`FlashSpec`/`ParticleSpec` sums three of these (tight/mid/wide, see
/// `default_glow_layers`) to build a light source that reads as a genuine white-hot core fading
/// through a tinted halo, rather than a single flat (possibly whitened) color at one spatial
/// scale — see `docs/fmstyle-format.md`'s "Brightness/overexposure" section for the full
/// before/after rationale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GlowLayer {
    pub amplitude: f32,
    pub sigma_px: f32,
}

/// Tune-by-eye default corona: a tight near-white core bloom, a mid halo, and a wide soft spread.
/// Not load-bearing — every consumer exposes this as the `layers` default so old/simple styles
/// get a reasonable glow for free, but any style can override it.
fn default_glow_layers() -> [GlowLayer; 3] {
    [
        GlowLayer {
            amplitude: 2.6,
            sigma_px: 5.0,
        },
        GlowLayer {
            amplitude: 1.1,
            sigma_px: 16.0,
        },
        GlowLayer {
            amplitude: 0.38,
            sigma_px: 48.0,
        },
    ]
}

/// Soft outer halo around a note's silhouette (or, since Phase K, the barrier bar too — this
/// struct is shared by `NoteLayer::glow` and `BarrierLayer::glow`). Since Phase M the halo itself
/// is an **additive** sum of `layers` (see `GlowLayer`'s doc comment) rather than a single
/// alpha-blended ring — this is what lets it read as light radiating from a bright core instead
/// of a flat lighter color. `brightness` scales how much light the corona adds — for the barrier's
/// own opaque bar (`BarrierLayer::glow`) it also still drives a `hot_color` desaturate-toward-white
/// mix on the bar itself (`barrier.wgsl`'s `fs_core`). Notes (`NoteLayer::glow`) used to have that
/// same white-hot-rim effect on their own opaque fill too; it was removed (see `shader.wgsl`'s
/// `fs_core` comment) because whitening the note's own fill read as an unwanted artifact. What
/// notes have instead: right at the boundary where the opaque fill meets the corona, the fill
/// blends toward `color * (sum of layer amplitudes) * brightness` (clamped to a displayable 0–1
/// range) — matching, not just this halo's raw color but its actual computed brightness right at
/// the edge, which is what the corona (`fs_glow`) itself evaluates to there — over
/// `edge_blend_px` pixels, so the fill's true color hands off continuously into the corona's
/// color/brightness instead of meeting it at a seam. This isn't a separate toggle; it's just how
/// `NoteLayer::glow` renders whenever `glow` is `Some(..)`. `brightness <= 1.0` behaves as a plain
/// dimmer, pushed past `1.0` the look reads as overexposure. `brightness = 1.0` is an exact no-op.
/// Unlike before Phase M, `brightness` no longer widens how far the corona reaches — reach is
/// purely `layers[i].sigma_px`-driven now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Glow {
    pub color: ColorBinding,
    #[serde(default = "default_brightness")]
    pub brightness: f32,
    #[serde(default = "default_glow_layers")]
    pub layers: [GlowLayer; 3],
    /// Distance (px), past the glowing surface's opaque edge, over which the note-edge rim (see
    /// this struct's own doc comment) blends from the note's true fill color to the corona's own
    /// color/brightness — independent of `layers[0].sigma_px` (the corona's own innermost falloff
    /// distance) so the rim's smoothness can be tuned without changing how far the corona itself
    /// visually reaches. `0.0` (default) falls back to `layers[0].sigma_px`, matching the behavior
    /// before this field existed. Larger values spread the handoff over more pixels (smoother,
    /// more gradual); smaller values make it snap to the corona's color more abruptly. Renderer-
    /// side: see `shader.wgsl`'s `fs_core` (notes) — not yet wired up for the barrier's own glow
    /// (`barrier.wgsl`), even though `Glow` is shared between `NoteLayer`/`BarrierLayer`.
    #[serde(default)]
    pub edge_blend_px: f32,
    /// When `true`, ignore `color` and instead tint the corona (and the note-edge rim) with the
    /// note's own fill color sampled at whichever point on the note's silhouette the corona
    /// fragment is closest to — so a note using `Fill::VerticalGradient`/`CanvasGradient` gets a
    /// halo that itself blends top-to-bottom (or across canvas height) matching the fill directly
    /// beneath it, rather than one fixed halo color for the whole note. **Only meaningful for
    /// `NoteLayer::glow`** — `BarrierLayer::glow` has no per-note fill to sample, so this field is
    /// a documented no-op there (`barrier.wgsl` never reads it), same precedent as
    /// `edge_blend_px` being notes-only. `false` (default) is an exact no-op, preserving every
    /// style's look from before this field existed.
    #[serde(default)]
    pub match_note_color: bool,
}

impl Default for Glow {
    fn default() -> Self {
        Self {
            color: ColorBinding::default(),
            brightness: 1.0,
            layers: default_glow_layers(),
            edge_blend_px: 0.0,
            match_note_color: false,
        }
    }
}

/// Schema-only this milestone (documented extension point) — parses and round-trips, but no
/// renderer draws it yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Border {
    pub color: ColorBinding,
    pub width_px: f32,
}

/// The falling notes themselves: fill plus optional sheen/glow/border layered on top, a
/// roundedness fraction, and the fall speed (see `NoteStyle`'s doc comment for why fall speed
/// also scales on-screen note length).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteLayer {
    pub fill: Fill,
    #[serde(default)]
    pub sheen: Option<Sheen>,
    #[serde(default)]
    pub glow: Option<Glow>,
    pub roundedness: f32,
    pub fall_speed: f32,
    #[serde(default)]
    pub border: Option<Border>,
    #[serde(default)]
    pub black_key_fill: BlackKeyFill,
}

impl Default for NoteLayer {
    fn default() -> Self {
        Self {
            fill: Fill::default(),
            sheen: None,
            glow: None,
            roundedness: 1.0,
            fall_speed: 400.0,
            border: None,
            black_key_fill: BlackKeyFill::default(),
        }
    }
}

/// Tune-by-eye default for `Pulse::brightness` (see its doc comment) — approximate/adjustable
/// once seen rendered, same convention as `effects.wgsl`'s existing `1.6` soft-glow exponent.
/// Picked so the shipped `barrier-pulse.fmstyle.ron` sample still reads as "brightens on hit"
/// without the sample author having to touch it, while a style can push further for a real
/// blowout.
fn default_pulse_brightness() -> f32 {
    1.6
}

/// Barrier brightens briefly when notes arrive, then decays back to its resting look (the
/// barrier's own `Glow::brightness` if it has one, else a plain `1.0`/no-op). `brightness` is the
/// peak color multiplier at the instant a note arrives, decaying linearly to that resting
/// baseline over `decay_seconds` — see `Glow`'s doc comment for the white-hot-core/corona
/// mechanism this shares. There used to be a separate `intensity` (0..1 peak amplitude) knob
/// multiplying into `brightness`; removed as a redundant axis — `brightness` alone is now the
/// peak, so what used to be `intensity: 0.8, brightness: 1.6` (peak effective multiplier `1.48`)
/// becomes simply `brightness: 1.48` (or whatever peak look is wanted) with no amplitude term.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pulse {
    pub decay_seconds: f32,
    #[serde(default = "default_pulse_brightness")]
    pub brightness: f32,
}

/// Which edges of the barrier ripple, and how the bottom edge (if it ripples at all) relates to
/// the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WavyMode {
    /// Only the top edge ripples; the bottom stays perfectly flat, so the bar's thickness varies
    /// across its width (the original "calm ocean cross-section" look).
    #[default]
    TopWave,
    /// The identical offset is applied to both edges, so the whole bar rides the wave rigidly —
    /// constant thickness, the bar translates as a unit. Reads as a thin curvy line rather than a
    /// bar with volume, since nothing about the bar's cross-section actually changes shape.
    Edge,
    /// Both edges bulge outward together (never inward past the base thickness), so the bar
    /// always has at least its configured thickness and swells further at wave crests on both
    /// sides at once — real, always-present volume with no pinch-to-nothing points, unlike an
    /// out-of-phase pairing that can cancel down to (near) zero thickness.
    FullWave,
}

/// Independent thin filament threads fraying off the barrier's wavy top edge, rendered inside the
/// corona (`fs_glow`) pass alongside the ordinary 3-layer glow — the SeeMusic-style look where the
/// top edge doesn't read as one smooth wavy line but several fine threads scattered just above it.
/// Ported from `explorations/barrier-fx-lab/barrier-fx-lab.html`'s "Wavy edge" strand controls
/// (the sliding-filament and wisp controls in that lab are a separate, not-yet-ported experiment —
/// see the lab's own `README.md`).
///
/// Each strand re-samples the same stochastic ripple (`barrier.wgsl`'s `wavy_offset_seeded`) with
/// its own seed derived from `jitter` — `0.0` makes every strand an identical copy of the main
/// edge, just offset in height; `1.0` fully decorrelates each strand's wiggle from the others and
/// from the main edge — and sits at its own fixed height above the real top edge, evenly spaced
/// from `0` (riding the edge itself) up to `spread_px` (the furthest-out strand). Each thread
/// renders as a thin exponential falloff (`thickness_px`) plus its own soft additive halo, using
/// one shared `halo_amplitude`/`halo_sigma_px` pair applied identically to every strand rather than
/// per-strand values.
///
/// Strands are tinted by the barrier's own `Glow::color`/brightness — there is no separate strand
/// color — and are rendered inside the corona pass, so **`BarrierLayer::glow` must be
/// `Some(..)` for strands to be visible**; `strands: Some(..)` with `glow: None` parses and
/// round-trips but renders nothing (no corona pass runs to draw them in).
///
/// **Only meaningful when `WavySpec::mode` is `Edge`.** `TopWave`/`FullWave` describe the bar's own
/// solid cross-section (its thickness rippling/swelling), not a bundle of independent threads
/// riding alongside a rigidly-translating edge — `barrier.wgsl`'s `fs_glow` checks the live `mode`
/// uniform and skips the whole strand loop outside `Edge`, even if `strands` is `Some(..)`; this is
/// the single source of truth — there is no matching CPU-side gate; `BarrierRenderer::set_style`
/// uploads strand params unconditionally whenever `strands` is `Some(..)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrandSpec {
    /// Number of independent threads, capped at 8 (`barrier.wgsl`'s loop bound — extra strands
    /// beyond 8 are silently ignored).
    #[serde(default = "default_strand_count")]
    pub count: u32,
    /// Height (canvas px) of the furthest-out strand above the real top edge; strands in between
    /// are spaced evenly from `0` (riding the edge) to this value.
    #[serde(default = "default_strand_spread_px")]
    pub spread_px: f32,
    /// `0.0` = every strand ripples in lockstep with the main edge, just offset in height; `1.0` =
    /// each strand's ripple is independently seeded and reads as fully decorrelated wiggle.
    #[serde(default = "default_strand_jitter")]
    pub jitter: f32,
    /// Thread thinness in px — smaller reads as a finer wire (`exp(-dist_px / thickness_px)`
    /// falloff around each strand's centerline).
    #[serde(default = "default_strand_thickness_px")]
    pub thickness_px: f32,
    /// Additive halo amplitude around each thread — same `amplitude * exp(-d / sigma)` falloff
    /// shape `GlowLayer` uses, but one shared pair applied identically to every strand rather than
    /// per-strand values.
    #[serde(default = "default_strand_halo_amplitude")]
    pub halo_amplitude: f32,
    /// Halo falloff distance in px.
    #[serde(default = "default_strand_halo_sigma_px")]
    pub halo_sigma_px: f32,
    /// Overall multiplier on the strand bundle's contribution to the corona.
    #[serde(default = "default_strand_glow_intensity")]
    pub glow_intensity: f32,
    /// How fast each strand's brightness flickers over transport time.
    #[serde(default = "default_strand_flicker_speed")]
    pub flicker_speed: f32,
}

impl Default for StrandSpec {
    fn default() -> Self {
        Self {
            count: default_strand_count(),
            spread_px: default_strand_spread_px(),
            jitter: default_strand_jitter(),
            thickness_px: default_strand_thickness_px(),
            halo_amplitude: default_strand_halo_amplitude(),
            halo_sigma_px: default_strand_halo_sigma_px(),
            glow_intensity: default_strand_glow_intensity(),
            flicker_speed: default_strand_flicker_speed(),
        }
    }
}

fn default_strand_count() -> u32 {
    5
}
fn default_strand_spread_px() -> f32 {
    14.0
}
fn default_strand_jitter() -> f32 {
    0.75
}
fn default_strand_thickness_px() -> f32 {
    1.4
}
fn default_strand_halo_amplitude() -> f32 {
    1.0
}
fn default_strand_halo_sigma_px() -> f32 {
    6.0
}
fn default_strand_glow_intensity() -> f32 {
    1.3
}
fn default_strand_flicker_speed() -> f32 {
    1.8
}

/// A calm, stochastic-looking (not a single literal sine) wavy edge for the barrier — three
/// incommensurate-frequency sine terms summed with weights 0.6/0.3/0.1 (see `barrier.wgsl`'s
/// `wavy_offset`), so `|offset| <= amplitude_px` always holds exactly.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WavySpec {
    /// Peak vertical displacement in canvas pixels.
    pub amplitude_px: f32,
    /// Pixels per cycle of the slowest (dominant) term.
    pub wavelength_px: f32,
    /// How fast the ripple pattern *mutates in place* over transport time — which parts of the
    /// noise field currently look big/small, not the field's x-position; `0` freezes the shape
    /// (still x-varying, not flat), but even a frozen shape sits at a fixed spot along x. See
    /// `slide_speed` for actual lateral translation.
    pub speed: f32,
    /// Which edges ripple and how. See `WavyMode`'s own doc comments.
    #[serde(default)]
    pub mode: WavyMode,
    /// How fast the ripple pattern's noise field itself translates sideways along the barrier's
    /// width, in canvas px/second — independent of `speed` (which mutates the pattern's shape in
    /// place, leaving its x-position fixed). A positive value gives a "current flowing through the
    /// wire" look: the whole ripple (and, since strands re-sample the same field, the whole strand
    /// bundle too) visibly crawls sideways rather than just wobbling in place. `0.0` (default) is
    /// an exact no-op — the field's x-position never moves, matching behavior before this field
    /// existed.
    #[serde(default)]
    pub slide_speed: f32,
    /// Independent filament threads riding just above the wavy top edge. See `StrandSpec`'s own
    /// doc comment for the full picture — in particular, only meaningful (rendered) when `mode` is
    /// `Edge`, and requires the barrier's `glow` to be `Some(..)` to actually be visible.
    #[serde(default)]
    pub strands: Option<StrandSpec>,
}

/// The horizontal barrier where falling notes stop. `glow` (Phase K) replaced the earlier
/// `kind: BarrierKind` + `glow_radius_px: f32` pair — presence of a `Glow` *is* the on/off switch
/// now (`None` = flat line, the only look before this phase), the same pattern `NoteLayer::glow`
/// already used. **Breaking change**: old `.fmstyle.ron` files with
/// `barrier: (kind: ..., glow_radius_px: ...)` need manual editing to the new
/// `glow: Some((color: ..., brightness: 1.0))` / `glow: None` shape — see
/// `docs/fmstyle-format.md`'s changelog. `show_bar` (Phase M) is independent of `glow` — whether
/// the flat/opaque bar itself renders at all, separate from whether it has a corona. Defaults to
/// `false` (an old `.fmstyle.ron` predating this field, or one that never bothered to set it,
/// gets pure glow with no visible bar) — the additive corona, not the flat opaque bar, is the look
/// this format is designed around; a style that actually wants the bar has to opt in explicitly.
/// A note has no equivalent field since a note without its own fill isn't a sensible look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarrierLayer {
    pub color: ColorBinding,
    pub thickness: f32,
    #[serde(default)]
    pub glow: Option<Glow>,
    #[serde(default)]
    pub pulse: Option<Pulse>,
    #[serde(default)]
    pub wavy: Option<WavySpec>,
    #[serde(default)]
    pub show_bar: bool,
}

impl Default for BarrierLayer {
    fn default() -> Self {
        Self {
            color: ColorBinding::default(),
            thickness: 4.0,
            glow: None,
            pulse: None,
            wavy: None,
            show_bar: false,
        }
    }
}

/// Which transition effect(s), if any, spawn when a note arrives at the barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TransitionKind {
    #[default]
    None,
    Particles,
    Flash,
    ParticlesAndFlash,
}

/// How particles are spawned relative to a note's held duration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum EmissionMode {
    /// Today's only behavior: `count` particles spawned once, at note arrival.
    #[default]
    Burst,
    /// Particles spawned continuously, every frame a note is held, spread across the width of
    /// its key — reads as the key being "ground down" rather than sparking once. `count` does
    /// not apply in this mode; particles/second is `rate_per_second`.
    Continuous { rate_per_second: f32 },
}

/// How a particle's color is chosen. `Fixed` is the original (pre-this-feature) behavior — every
/// particle from a spec gets the same resolved color. `MatchNoteBottom` and `YGradient` are a
/// single mutually-exclusive mode selector (not independent toggles), since a particle's color has
/// to come from exactly one source:
/// - `MatchNoteBottom`: every particle from a given note gets that note's own already-resolved
///   bottom gradient endpoint (`render::notes::NoteInterval::color_bottom` — the exact value baked
///   into that note's own `color_bottom`), not a finer per-pixel sample of its actual rendered
///   fill/sheen. One color per note (not per particle), so it stays correct for any current or
///   future `Fill` without needing anything ported to Rust — see `docs/fmstyle-format.md`'s
///   "Note-bottom color sampling" section for the tradeoff this makes.
/// - `YGradient`: particles are tinted by their own *current* canvas Y position (top of frame ->
///   `top`, barrier line -> `bottom`), the same span `Fill::CanvasGradient` blends notes across —
///   unlike `Fixed`/`MatchNoteBottom` (baked once at spawn), this is recomputed every frame as a
///   particle falls/rises, so a particle visibly shifts color as it moves through the scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParticleColor {
    Fixed(ColorBinding),
    MatchNoteBottom,
    YGradient {
        top: ColorBinding,
        bottom: ColorBinding,
    },
}

impl Default for ParticleColor {
    fn default() -> Self {
        ParticleColor::Fixed(ColorBinding::default())
    }
}

/// Fixed-pool particle burst spawned on note arrival (MIDIVisualizer-style: spawn, fade, expire —
/// no external texture asset, rendered as a procedural radial sprite).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParticleSpec {
    pub count: u32,
    pub lifetime_seconds: f32,
    pub size_px: f32,
    pub speed_px: f32,
    pub spread_degrees: f32,
    pub gravity_px: f32,
    #[serde(default)]
    pub color: ParticleColor,
    pub additive: bool,
    #[serde(default)]
    pub emission: EmissionMode,
    /// Color multiplier applied at spawn time (Phase K) — see `Glow`'s doc comment for the
    /// overdrive mechanism; `1.0` is a no-op, reproducing every particle look that existed before
    /// this field did.
    #[serde(default = "default_brightness")]
    pub brightness: f32,
    /// Additive corona layers (Phase M), same mechanism and default as `Glow::layers` — only
    /// meaningful when `additive` is true (a non-additive "puff" particle never reads this field).
    #[serde(default = "default_glow_layers")]
    pub layers: [GlowLayer; 3],
}

/// When a spawned flash starts decaying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FlashMode {
    /// Today's only behavior: decays over `decay_seconds` starting immediately at note-on.
    #[default]
    Instant,
    /// Holds at full intensity for as long as the note is held, decaying over `decay_seconds`
    /// only after the note ends — a glow triggered by the key press rather than a quick pulse,
    /// matching the seemusic/Synthesia look.
    Sustained,
}

/// How a flash's color varies across its own footprint. A flash always renders as an ellipse
/// (`radius_x_px`/`radius_y_px`); this enum controls whether that ellipse is one flat color or a
/// horizontal multi-stop gradient, either hand-authored or auto-derived from a note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FlashColor {
    /// One flat color, resolved once — today's behavior before this feature existed.
    Solid(ColorBinding),
    /// A hand-authored horizontal gradient: evenly spaced left-to-right color stops across the
    /// flash's own width (`2 * radius_x_px`). Any number of stops (including 1, equivalent to
    /// `Solid`) is accepted; the renderer resamples this list to its fixed internal stop count at
    /// spawn time.
    HorizontalGradient(Vec<ColorBinding>),
    /// Auto-derived from the note that triggered this flash: one flat color, the note's own
    /// already-resolved bottom gradient endpoint (`render::notes::NoteInterval::color_bottom`) —
    /// the same value `ParticleColor::MatchNoteBottom` uses, not a finer per-pixel sample of the
    /// note's actual rendered fill/sheen. See `docs/fmstyle-format.md`'s "Note-bottom color
    /// sampling" section. For a genuinely multicolor flash, use `HorizontalGradient` instead.
    MatchNoteBottom,
}

impl Default for FlashColor {
    fn default() -> Self {
        FlashColor::Solid(ColorBinding::default())
    }
}

/// Decaying radial flash spawned on note arrival. Since a flash always renders additively, its
/// peak opacity and its color-brightness had the exact same visual effect (both just scale the
/// additive contribution) — the old separate `intensity` (peak alpha) knob was redundant with
/// `brightness` and has been removed; a flash is always fully opaque at spawn (fading to 0 over
/// `decay_seconds`, as before) and `brightness` alone controls how hot/white it looks, same
/// mechanism as `Glow`'s doc comment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlashSpec {
    pub radius_x_px: f32,
    pub radius_y_px: f32,
    #[serde(default)]
    pub color: FlashColor,
    pub decay_seconds: f32,
    #[serde(default)]
    pub mode: FlashMode,
    /// Color multiplier applied at spawn time (Phase K) — see `Glow`'s doc comment for the
    /// overdrive mechanism; `1.0` is a no-op, reproducing every flash look that existed before
    /// this field did (including `FlashMode::Sustained`'s "key glow" look).
    #[serde(default = "default_brightness")]
    pub brightness: f32,
    /// Additive corona layers (Phase M), same mechanism and default as `Glow::layers` — a flash
    /// always renders additively, so this always applies.
    #[serde(default = "default_glow_layers")]
    pub layers: [GlowLayer; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TransitionLayer {
    pub kind: TransitionKind,
    #[serde(default)]
    pub particles: Option<ParticleSpec>,
    #[serde(default)]
    pub flash: Option<FlashSpec>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_resolve_returns_the_static_value_at_any_time() {
        let timed = Timed::Static(42);
        assert_eq!(*timed.resolve(0.0), 42);
        assert_eq!(*timed.resolve(-100.0), 42);
        assert_eq!(*timed.resolve(1000.0), 42);
    }

    #[test]
    fn timed_resolve_keyed_picks_last_key_at_or_before_t_and_clamps_before_the_first() {
        let timed = Timed::Keyed(vec![(0.0, "a"), (10.0, "b"), (20.0, "c")]);
        assert_eq!(*timed.resolve(-5.0), "a");
        assert_eq!(*timed.resolve(0.0), "a");
        assert_eq!(*timed.resolve(9.999), "a");
        assert_eq!(*timed.resolve(10.0), "b");
        assert_eq!(*timed.resolve(15.0), "b");
        assert_eq!(*timed.resolve(20.0), "c");
        assert_eq!(*timed.resolve(1000.0), "c");
    }

    #[test]
    fn color_binding_constant_resolves_exactly() {
        let binding = ColorBinding::Constant([10, 20, 30]);
        assert_eq!(binding.resolve_constant(), [10, 20, 30]);
    }

    #[test]
    fn color_binding_non_constant_falls_back_to_a_representative_constant() {
        assert_eq!(
            ColorBinding::ByVelocity(Ramp {
                low: [0, 0, 0],
                high: [255, 0, 0]
            })
            .resolve_constant(),
            [255, 0, 0]
        );
        assert_eq!(
            ColorBinding::ByTrack(vec![]).resolve_constant(),
            [255, 255, 255]
        );
    }

    #[test]
    fn style_ron_round_trip() {
        let style = Style::from_legacy(&NoteStyle::default(), &BarrierStyle::default(), [0, 0, 0]);
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    #[test]
    fn black_key_fill_custom_gradient_round_trips() {
        let mut style =
            Style::from_legacy(&NoteStyle::default(), &BarrierStyle::default(), [0, 0, 0]);
        let Timed::Static(notes) = &mut style.notes else {
            unreachable!()
        };
        notes.black_key_fill = BlackKeyFill::Custom(Fill::VerticalGradient {
            top: ColorBinding::Constant([10, 20, 30]),
            bottom: ColorBinding::Constant([1, 2, 3]),
        });
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    /// `Fill::CanvasGradient` (canvas-Y-position note gradient) round-trips, including as a
    /// `black_key_fill` override independent of the natural-key fill's own variant.
    #[test]
    fn canvas_gradient_fill_round_trips() {
        let mut style =
            Style::from_legacy(&NoteStyle::default(), &BarrierStyle::default(), [0, 0, 0]);
        let Timed::Static(notes) = &mut style.notes else {
            unreachable!()
        };
        notes.fill = Fill::CanvasGradient {
            top: ColorBinding::Constant([200, 220, 255]),
            bottom: ColorBinding::Constant([10, 20, 60]),
        };
        notes.black_key_fill = BlackKeyFill::Custom(Fill::CanvasGradient {
            top: ColorBinding::Constant([100, 110, 130]),
            bottom: ColorBinding::Constant([5, 10, 30]),
        });
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    #[test]
    fn style_default_round_trips_too() {
        let style = Style::default();
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    #[test]
    fn style_background_round_trips_with_a_non_default_color() {
        let mut style =
            Style::from_legacy(&NoteStyle::default(), &BarrierStyle::default(), [0, 0, 0]);
        style.background = ColorBinding::Constant([12, 34, 56]);
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    /// An old `.fmstyle.ron` file predating `background` (or one that just never sets it) has no
    /// `background` key at all — `serde(default)` should load it as black, matching the hardcoded
    /// clear color every renderer used before this field existed, not an arbitrary fallback.
    #[test]
    fn style_without_background_field_loads_as_black() {
        let text = "(notes: Static((fill: Solid(Constant((1, 2, 3))), roundedness: 1.0, fall_speed: 400.0)))";
        let style: Style = ron::from_str(text).unwrap();
        assert_eq!(style.background, ColorBinding::Constant([0, 0, 0]));
    }

    /// Phase K: `BarrierLayer::glow` (replacing the earlier `kind`/`glow_radius_px` pair) and the
    /// new `brightness` fields on `Pulse`/`FlashSpec`/`ParticleSpec` round-trip through RON.
    #[test]
    fn barrier_layer_with_glow_and_pulse_brightness_round_trips() {
        let mut style =
            Style::from_legacy(&NoteStyle::default(), &BarrierStyle::default(), [0, 0, 0]);
        let Timed::Static(barrier) = &mut style.barrier else {
            unreachable!()
        };
        barrier.glow = Some(Glow {
            color: ColorBinding::Constant([255, 220, 120]),
            brightness: 3.0,
            layers: default_glow_layers(),
            edge_blend_px: 0.0,
            match_note_color: false,
        });
        barrier.pulse = Some(Pulse {
            decay_seconds: 0.35,
            brightness: 4.0,
        });
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    #[test]
    fn barrier_layer_without_glow_round_trips() {
        let style = Style::from_legacy(&NoteStyle::default(), &BarrierStyle::default(), [0, 0, 0]);
        let Timed::Static(barrier) = &style.barrier else {
            unreachable!()
        };
        assert_eq!(barrier.glow, None);
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    #[test]
    fn transition_layer_brightness_fields_round_trip() {
        let style = Style {
            version: 1,
            notes: Timed::default(),
            barrier: Timed::default(),
            transition: Timed::Static(TransitionLayer {
                kind: TransitionKind::ParticlesAndFlash,
                particles: Some(ParticleSpec {
                    count: 10,
                    lifetime_seconds: 0.4,
                    size_px: 4.0,
                    speed_px: 180.0,
                    spread_degrees: 60.0,
                    gravity_px: 300.0,
                    color: ParticleColor::Fixed(ColorBinding::Constant([255, 240, 200])),
                    additive: true,
                    emission: EmissionMode::Burst,
                    brightness: 5.0,
                    layers: default_glow_layers(),
                }),
                flash: Some(FlashSpec {
                    radius_x_px: 40.0,
                    radius_y_px: 40.0,
                    color: FlashColor::Solid(ColorBinding::Constant([255, 255, 255])),
                    decay_seconds: 0.15,
                    mode: FlashMode::Instant,
                    brightness: 6.0,
                    layers: default_glow_layers(),
                }),
            }),
            background: default_background_color(),
        };
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    /// Old `.fmstyle.ron` files predating Phase K's `brightness` field (and Phase M's `layers`
    /// field) have neither key on `Glow`/`Pulse`/`FlashSpec`/`ParticleSpec` — `serde(default)`
    /// should load them with the documented defaults rather than failing to parse.
    #[test]
    fn glow_without_brightness_field_loads_with_default() {
        let text = "(color: Constant((1, 2, 3)))";
        let glow: Glow = ron::from_str(text).unwrap();
        assert_eq!(glow.brightness, 1.0);
        assert_eq!(glow.layers, default_glow_layers());
        assert_eq!(glow.edge_blend_px, 0.0);
    }

    #[test]
    fn pulse_without_brightness_field_loads_with_tuned_default() {
        let text = "(decay_seconds: 0.35)";
        let pulse: Pulse = ron::from_str(text).unwrap();
        assert_eq!(pulse.brightness, 1.6);
    }

    /// Confirms RON actually round-trips a fixed-size `[GlowLayer; 3]` array with non-default
    /// values (not just the shared `default_glow_layers()` every other test above happens to use)
    /// — `ron` serializes fixed-size arrays with tuple parens `layers: (...)`, not brackets
    /// `layers: [...]`, easy to get wrong hand-editing a sample file, so this is worth checking
    /// empirically rather than assuming.
    #[test]
    fn glow_layers_array_with_explicit_values_round_trips() {
        let glow = Glow {
            color: ColorBinding::Constant([10, 20, 30]),
            brightness: 2.5,
            layers: [
                GlowLayer {
                    amplitude: 1.0,
                    sigma_px: 2.0,
                },
                GlowLayer {
                    amplitude: 3.0,
                    sigma_px: 4.0,
                },
                GlowLayer {
                    amplitude: 5.0,
                    sigma_px: 6.0,
                },
            ],
            edge_blend_px: 3.0,
            match_note_color: false,
        };
        let text = ron::ser::to_string_pretty(&glow, ron::ser::PrettyConfig::new()).unwrap();
        assert!(
            text.contains("layers: ("),
            "expected tuple-paren array syntax, got: {text}"
        );
        let parsed: Glow = ron::from_str(&text).unwrap();
        assert_eq!(glow, parsed);
    }

    /// `show_bar` (Phase M) is a plain `#[serde(default)]` `bool`, so it defaults to `false` when
    /// a `.fmstyle.ron` predates the field or just never sets it explicitly — an old/simple style
    /// gets pure corona with no visible opaque bar unless it opts in with `show_bar: true`.
    #[test]
    fn barrier_layer_show_bar_defaults_to_false_when_omitted() {
        let text = "(color: Constant((1, 2, 3)), thickness: 4.0)";
        let barrier: BarrierLayer = ron::from_str(text).unwrap();
        assert!(!barrier.show_bar);
    }

    /// `Glow::match_note_color` round-trips and defaults to `false` when an older `.fmstyle.ron`
    /// fragment omits it entirely.
    #[test]
    fn glow_match_note_color_round_trips_and_defaults_to_false() {
        let glow = Glow {
            match_note_color: true,
            ..Glow::default()
        };
        let text = ron::ser::to_string_pretty(&glow, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Glow = ron::from_str(&text).unwrap();
        assert_eq!(glow, parsed);

        let old_text = "(color: Constant((1, 2, 3)))";
        let old: Glow = ron::from_str(old_text).unwrap();
        assert!(!old.match_note_color);
    }

    /// `ParticleColor`'s three variants (the single mutually-exclusive mode selector — see its own
    /// doc comment for why `MatchNoteBottom`/`YGradient` aren't independent toggles) round-trip.
    #[test]
    fn particle_color_variants_round_trip() {
        for color in [
            ParticleColor::Fixed(ColorBinding::Constant([1, 2, 3])),
            ParticleColor::MatchNoteBottom,
            ParticleColor::YGradient {
                top: ColorBinding::Constant([10, 20, 30]),
                bottom: ColorBinding::Constant([200, 210, 220]),
            },
        ] {
            let spec = ParticleSpec {
                count: 1,
                lifetime_seconds: 0.5,
                size_px: 2.0,
                speed_px: 100.0,
                spread_degrees: 30.0,
                gravity_px: 200.0,
                color: color.clone(),
                additive: true,
                emission: EmissionMode::Burst,
                brightness: 1.0,
                layers: default_glow_layers(),
            };
            let text = ron::ser::to_string_pretty(&spec, ron::ser::PrettyConfig::new()).unwrap();
            let parsed: ParticleSpec = ron::from_str(&text).unwrap();
            assert_eq!(spec, parsed);
        }
    }

    /// `FlashColor`'s three variants (solid, an author-painted multi-stop gradient, and the
    /// auto-derived note-bottom cross-section) round-trip.
    #[test]
    fn flash_color_variants_round_trip() {
        for color in [
            FlashColor::Solid(ColorBinding::Constant([1, 2, 3])),
            FlashColor::HorizontalGradient(vec![
                ColorBinding::Constant([255, 0, 0]),
                ColorBinding::Constant([0, 255, 0]),
                ColorBinding::Constant([0, 0, 255]),
            ]),
            FlashColor::MatchNoteBottom,
        ] {
            let spec = FlashSpec {
                radius_x_px: 20.0,
                radius_y_px: 20.0,
                color: color.clone(),
                decay_seconds: 0.2,
                mode: FlashMode::Instant,
                brightness: 1.0,
                layers: default_glow_layers(),
            };
            let text = ron::ser::to_string_pretty(&spec, ron::ser::PrettyConfig::new()).unwrap();
            let parsed: FlashSpec = ron::from_str(&text).unwrap();
            assert_eq!(spec, parsed);
        }
    }

    /// Guards the shipped `examples/styles/*.fmstyle.ron` samples against drifting out of sync
    /// with the schema — each one should still parse as a `Style` via the same `Style::load` path
    /// the "Import style…" button uses.
    #[test]
    fn shipped_sample_styles_parse() {
        let styles_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/styles");
        let mut checked = 0;
        for entry in std::fs::read_dir(&styles_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("ron") {
                continue;
            }
            Style::load(&path).unwrap_or_else(|err| panic!("failed to parse {path:?}: {err}"));
            checked += 1;
        }
        assert!(
            checked >= 3,
            "expected at least 3 sample styles, found {checked}"
        );
    }
}
