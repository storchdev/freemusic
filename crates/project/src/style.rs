//! `.fmstyle.ron` format: a data-driven description of note/barrier/transition visuals. Every
//! field is `#[serde(default)]`-compatible via the wrapper types below, so the schema can grow new
//! fields without breaking existing files. This module only defines the schema and its resolution
//! helpers — see `crates/render` for the renderer that consumes it.
//!
//! For the field-by-field contract (defaults, meaning, RON snippets, breaking-change log), see
//! `docs/fmstyle-format.md` — keep it in sync whenever this module's schema changes.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{BarrierStyle, NoteStyle};

fn current_style_version() -> u32 {
    1
}

/// Black — the canvas background for a `.fmstyle.ron` that doesn't set `background` explicitly
/// (or the no-imported-style legacy path, see `from_legacy`).
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
    /// `Fill::Solid`, no sheen/glow, no barrier glow, `TransitionKind::None` — so the renderer
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
                alpha: ScalarBinding::default(),
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

/// A per-note color: fixed, or driven by the note's own velocity, pitch class, absolute pitch, or
/// track index — see `resolve_for_note`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ColorBinding {
    Constant([u8; 3]),
    ByVelocity(Ramp),
    ByPitchClass([[u8; 3]; 12]),
    /// Scales continuously across the *whole* keyboard (unlike `ByPitchClass`, which repeats
    /// every octave via `pitch % 12`) — see `pitch_fraction`'s doc comment for the range this is
    /// keyed against.
    ByPitch(Ramp),
    ByTrack(Vec<[u8; 3]>),
}

fn lerp_color(low: [u8; 3], high: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (low[0] as f32 + (high[0] as f32 - low[0] as f32) * t).round() as u8,
        (low[1] as f32 + (high[1] as f32 - low[1] as f32) * t).round() as u8,
        (low[2] as f32 + (high[2] as f32 - low[2] as f32) * t).round() as u8,
    ]
}

/// Lowest/highest MIDI pitch of a standard 88-key piano (A0/C8) — `piano_layout::KeyboardRange::
/// standard_88_keys()`'s own bounds, duplicated here (rather than taking a dependency on
/// `piano-layout` from this crate) since this is the only place `project` needs them. Hardcoded
/// because the app's keyboard range isn't adjustable yet; if/when it becomes a per-project
/// setting, `ByPitch`/`pitch_fraction` will need to key off that instead of this constant.
const STANDARD_88_KEY_LOW: u8 = 21;
const STANDARD_88_KEY_HIGH: u8 = 108;

/// Where `pitch` sits within the standard 88-key range, `0.0` at the lowest key (A0) to `1.0` at
/// the highest (C8), clamped for any pitch outside that range. Shared by `ColorBinding::ByPitch`
/// and `ScalarBinding::ByPitch` — unlike `ByPitchClass` (which wraps every octave identically via
/// `pitch % 12`, so every C reads the same regardless of register), this scales across the *whole*
/// keyboard, so the lowest and highest notes are unambiguously different colors/values.
fn pitch_fraction(pitch: u8) -> f32 {
    let span = (STANDARD_88_KEY_HIGH - STANDARD_88_KEY_LOW) as f32;
    (pitch.saturating_sub(STANDARD_88_KEY_LOW) as f32 / span).clamp(0.0, 1.0)
}

impl ColorBinding {
    /// Resolves against a specific note's velocity (0-127), MIDI pitch number (0-127), and track
    /// index — the real per-note resolution the property-driven variants are meant for:
    /// - `Constant` ignores all three and returns its fixed color.
    /// - `ByVelocity(ramp)` linearly interpolates `ramp.low` (velocity 0) to `ramp.high`
    ///   (velocity 127).
    /// - `ByPitchClass(colors)` indexes by `pitch % 12` (0 = C, 1 = C#, ... 11 = B, independent of
    ///   octave).
    /// - `ByPitch(ramp)` linearly interpolates `ramp.low` (lowest key, A0) to `ramp.high` (highest
    ///   key, C8) — see `pitch_fraction`.
    /// - `ByTrack(colors)` indexes by `track_id % colors.len()` (wrapping so a style authored for
    ///   fewer colors than a MIDI file has tracks still resolves deterministically instead of
    ///   panicking), or white if `colors` is empty.
    ///
    /// Use this wherever an actual note is in scope (note fill, particle/flash colors keyed to a
    /// note); use `resolve_constant` for genuinely note-less contexts (canvas background, the
    /// barrier bar/glow, the note-glow GPU uniform — see its own doc comment for why those can't
    /// vary per note).
    pub fn resolve_for_note(&self, velocity: u8, pitch: u8, track_id: usize) -> [u8; 3] {
        match self {
            ColorBinding::Constant(color) => *color,
            ColorBinding::ByVelocity(ramp) => {
                lerp_color(ramp.low, ramp.high, velocity as f32 / 127.0)
            }
            ColorBinding::ByPitchClass(colors) => colors[(pitch % 12) as usize],
            ColorBinding::ByPitch(ramp) => lerp_color(ramp.low, ramp.high, pitch_fraction(pitch)),
            ColorBinding::ByTrack(colors) => {
                if colors.is_empty() {
                    [255, 255, 255]
                } else {
                    colors[track_id % colors.len()]
                }
            }
        }
    }

    /// Resolves to a single representative color, for contexts with no specific note to key
    /// off of: exact for `Constant`; for the property-driven variants, a documented
    /// representative value (`ByVelocity`/`ByPitch`'s high end, `ByPitchClass`'s first entry,
    /// `ByTrack`'s first entry or white if empty) — **not** a "not yet wired up" placeholder,
    /// these contexts (canvas background, the barrier bar/glow, the note-glow GPU uniform shared
    /// by every note) structurally have no single note to resolve against. Prefer
    /// `resolve_for_note` wherever an actual note is in scope.
    pub fn resolve_constant(&self) -> [u8; 3] {
        match self {
            ColorBinding::Constant(color) => *color,
            ColorBinding::ByVelocity(ramp) => ramp.high,
            ColorBinding::ByPitchClass(colors) => colors[0],
            ColorBinding::ByPitch(ramp) => ramp.high,
            ColorBinding::ByTrack(colors) => colors.first().copied().unwrap_or([255, 255, 255]),
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

/// A per-note scalar: fixed, or driven by the note's own velocity, pitch class, or track index —
/// same shape and resolution rules as `ColorBinding`. Used by `ParticleSpec::brightness`/
/// `FlashSpec::brightness` (both resolved per triggering note — see `resolve_for_note`); not used
/// by `Glow::brightness`/`Pulse::brightness` (those stay a plain `f32` — see their own doc
/// comments for why: one GPU uniform / one canvas-wide bar, no single note to resolve against).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScalarBinding {
    Constant(f32),
    ByVelocity {
        low: f32,
        high: f32,
    },
    ByPitchClass([f32; 12]),
    /// Scales continuously across the *whole* keyboard — see `ColorBinding::ByPitch`'s doc
    /// comment (same `pitch_fraction` mechanism, numeric endpoints instead of colors).
    ByPitch {
        low: f32,
        high: f32,
    },
    ByTrack(Vec<f32>),
}

impl ScalarBinding {
    /// Resolves against a specific note's velocity/pitch/track index — see `ColorBinding::
    /// resolve_for_note`, whose five-case mapping this mirrors exactly (`ByVelocity` interpolates
    /// `low`->`high` by `velocity / 127`; `ByPitchClass` indexes `pitch % 12`; `ByPitch`
    /// interpolates `low`->`high` across the whole 88-key range (`pitch_fraction`); `ByTrack`
    /// indexes `track_id % len`, wrapping, or falls back to `1.0` if empty).
    pub fn resolve_for_note(&self, velocity: u8, pitch: u8, track_id: usize) -> f32 {
        match self {
            ScalarBinding::Constant(value) => *value,
            ScalarBinding::ByVelocity { low, high } => {
                let t = (velocity as f32 / 127.0).clamp(0.0, 1.0);
                low + (high - low) * t
            }
            ScalarBinding::ByPitchClass(values) => values[(pitch % 12) as usize],
            ScalarBinding::ByPitch { low, high } => low + (high - low) * pitch_fraction(pitch),
            ScalarBinding::ByTrack(values) => {
                if values.is_empty() {
                    1.0
                } else {
                    values[track_id % values.len()]
                }
            }
        }
    }

    /// Resolves to a single representative value, for contexts with no specific note to key off
    /// of — see `ColorBinding::resolve_constant`'s doc comment for the same reasoning. Currently
    /// unused (every field that reads `ScalarBinding` always has a note to resolve against), kept
    /// for symmetry with `ColorBinding` and for whichever future note-less scalar field needs it.
    pub fn resolve_constant(&self) -> f32 {
        match self {
            ScalarBinding::Constant(value) => *value,
            ScalarBinding::ByVelocity { high, .. } => *high,
            ScalarBinding::ByPitchClass(values) => values[0],
            ScalarBinding::ByPitch { high, .. } => *high,
            ScalarBinding::ByTrack(values) => values.first().copied().unwrap_or(1.0),
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

/// `0.0`/`0.8` approximates "top of frame -> barrier line" (rather than e.g. `0.0`/`1.0`), absent
/// a real per-project barrier position to default to — see `ParticleColor::YGradient`'s doc
/// comment for the practical implications of this default span.
fn default_y_gradient_top_fraction() -> f32 {
    0.0
}

fn default_y_gradient_bottom_fraction() -> f32 {
    0.8
}

/// One exponential falloff term in an additive corona sum: `amplitude * exp(-d / sigma_px)`,
/// where `d` is distance outside the glowing surface's opaque edge. A `Glow`/`FlashSpec`/
/// `ParticleSpec` sums three of these (tight/mid/wide, see `default_glow_layers`) to build a
/// light source that reads as a genuine white-hot core fading through a tinted halo, rather than
/// a single flat (possibly whitened) color at one spatial scale — see
/// `docs/fmstyle-format.md`'s "Brightness/overexposure" section for the formula and design
/// history in `docs/fmstyle-history.md`.
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

/// Shared `ScalarBinding` default for fields where `0.0` means "off"/no-op (unlike
/// `ScalarBinding::default()`'s own `Constant(1.0)`, tuned for multiplier fields instead) — e.g.
/// `FlashSpec::flicker_speed`/`flicker_intensity`.
fn default_zero_scalar() -> ScalarBinding {
    ScalarBinding::Constant(0.0)
}

/// Soft outer halo around a note's silhouette, or the barrier bar's — this struct is shared by
/// `NoteLayer::glow` and `BarrierLayer::glow`. The halo itself is an **additive** sum of `layers`
/// (see `GlowLayer`'s doc comment) rather than a single alpha-blended ring — this is what lets it
/// read as light radiating from a bright core instead of a flat lighter color. `brightness` scales
/// how much light the corona adds — for the barrier's own opaque bar (`BarrierLayer::glow`) it
/// also drives a `hot_color` desaturate-toward-white mix on the bar itself (`barrier.wgsl`'s
/// `fs_core`). Notes (`NoteLayer::glow`) don't whiten their own opaque fill the same way (see
/// `docs/fmstyle-history.md`'s "Glow and brightness design" for why); instead, right at the
/// boundary where the opaque fill meets the corona, the fill blends toward
/// `color * (sum of layer amplitudes) * brightness` (clamped to a displayable 0–1 range) —
/// matching not just this halo's raw color but its actual computed brightness right at the edge,
/// which is what the corona (`fs_glow`) itself evaluates to there — over `edge_blend_px` pixels,
/// so the fill's true color hands off continuously into the corona's color/brightness instead of
/// meeting it at a seam. This isn't a separate toggle; it's just how `NoteLayer::glow` renders
/// whenever `glow` is `Some(..)`. `brightness <= 1.0` behaves as a plain dimmer, pushed past `1.0`
/// the look reads as overexposure. `brightness = 1.0` is an exact no-op. `brightness` does not
/// affect how far the corona reaches — reach is purely `layers[i].sigma_px`-driven.
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
    /// visually reaches. `0.0` (default) falls back to `layers[0].sigma_px`. Larger values spread
    /// the handoff over more pixels (smoother, more gradual); smaller values make it snap to the
    /// corona's color more abruptly. Renderer-
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
    /// `edge_blend_px` being notes-only. `false` (default) is an exact no-op.
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
    /// Note opacity, resolved per note like `ColorBinding` (`ByVelocity`/`ByPitchClass`/`ByTrack`
    /// can make quieter or specific notes more transparent). `1.0` (opaque, the default) is a
    /// pixel-identical no-op — the note core pipeline is already alpha-blended (for the rounded-
    /// corner antialiasing edge), so this only multiplies that edge coverage by the resolved
    /// value in `fs_core` (`shader.wgsl`) rather than needing a new blend state. Applies to the
    /// note's own fill only; the glow corona (`fs_glow`, additive) is unaffected.
    #[serde(default)]
    pub alpha: ScalarBinding,
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
            alpha: ScalarBinding::default(),
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
/// mechanism this shares.
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
    /// an exact no-op — the field's x-position never moves.
    #[serde(default)]
    pub slide_speed: f32,
    /// Independent filament threads riding just above the wavy top edge. See `StrandSpec`'s own
    /// doc comment for the full picture — in particular, only meaningful (rendered) when `mode` is
    /// `Edge`, and requires the barrier's `glow` to be `Some(..)` to actually be visible.
    #[serde(default)]
    pub strands: Option<StrandSpec>,
}

/// The horizontal barrier where falling notes stop. Presence of a `Glow` on `glow` is the on/off
/// switch for the corona (`None` = flat line), the same pattern `NoteLayer::glow` uses — see
/// `docs/fmstyle-history.md`'s breaking-change log for the older `kind`/`glow_radius_px` shape
/// this replaced. `show_bar` is independent of `glow` — whether the flat/opaque bar itself
/// renders at all, separate from whether it has a corona. Defaults to `false` — the additive
/// corona, not the flat opaque bar, is the look this format is designed around; a style that
/// wants the bar has to opt in explicitly. A note has no equivalent field since a note without
/// its own fill isn't a sensible look.
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

/// How a particle's color is chosen. `Fixed` gives every particle from a spec the same resolved
/// color. `MatchNote` and `YGradient` are a single mutually-exclusive mode selector (not
/// independent toggles), since a particle's color has to come from exactly one source:
/// - `MatchNote`: every particle spawned for a given note is colored by that note's fill at
///   whichever point is currently crossing the barrier (`render::notes::NoteInterval::
///   color_at_barrier`) — the leading edge's color right at arrival, sliding toward the trailing
///   edge's color as the note is held (relevant under `EmissionMode::Continuous`, which spawns
///   particles continuously for as long as a note is held, rather than once at arrival). Resolved
///   once per note per frame (not a finer per-pixel sample of its actual rendered fill/sheen), so
///   it stays correct for any current or future `Fill` without needing anything ported to Rust —
///   see `docs/fmstyle-format.md`'s "Note color sampling at the barrier" section for the tradeoff
///   this makes.
/// - `YGradient`: particles are tinted by their own *current* canvas Y position, blended between
///   `top` and `bottom` across the span `[top_fraction, bottom_fraction]` (each a fraction of
///   canvas height, `0.0` = top of frame, `1.0` = bottom) — unlike `Fixed`/`MatchNote` (baked once
///   at spawn), this is recomputed every frame as a particle falls/rises, so a particle visibly
///   shifts color as it moves through the scene. `top_fraction`/`bottom_fraction` default to
///   `0.0`/`0.8` (top of frame to a typical default barrier position), but since particles spawn
///   at the barrier and rarely travel far from it, that default span is usually far wider than
///   where particles actually live — most of their travel maps to a narrow sliver near `t == 1.0`,
///   so the color barely changes. Narrowing the span to bracket where particles actually travel
///   (e.g. just above and below the barrier) makes the gradient actually visible across a
///   particle's motion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParticleColor {
    Fixed(ColorBinding),
    MatchNote,
    YGradient {
        top: ColorBinding,
        bottom: ColorBinding,
        #[serde(default = "default_y_gradient_top_fraction")]
        top_fraction: f32,
        #[serde(default = "default_y_gradient_bottom_fraction")]
        bottom_fraction: f32,
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
    /// Resolved per triggering note (`ScalarBinding::resolve_for_note`, same mechanism as
    /// `brightness` below) — e.g. a harder hit can spawn longer-lived, bigger, faster, more
    /// widely-spread, and/or more-gravity-affected particles instead of every note's burst having
    /// identical shape. **Breaking change**: an older `.fmstyle.ron` with a bare float on any of
    /// these five fields (e.g. `lifetime_seconds: 0.4`) needs updating to
    /// `lifetime_seconds: Constant(0.4)` — see `docs/fmstyle-format.md`'s migration history.
    pub lifetime_seconds: ScalarBinding,
    pub size_px: ScalarBinding,
    pub speed_px: ScalarBinding,
    pub spread_degrees: ScalarBinding,
    pub gravity_px: ScalarBinding,
    #[serde(default)]
    pub color: ParticleColor,
    pub additive: bool,
    #[serde(default)]
    pub emission: EmissionMode,
    /// Color multiplier applied at spawn time — see `Glow`'s doc comment for the overdrive
    /// mechanism; `Constant(1.0)` is a no-op. Resolved once per spawned burst/continuous-emission
    /// tick against the *triggering* note's velocity/pitch/track (`render::effects`'s
    /// `spawn_particles`/continuous-emission loop), same mechanism as `ParticleColor::Fixed`'s
    /// `ColorBinding`. **Breaking change**: an older `.fmstyle.ron` with a bare float here (e.g.
    /// `brightness: 1.0`) needs updating to `brightness: Constant(1.0)` — see
    /// `docs/fmstyle-format.md`'s migration history.
    #[serde(default)]
    pub brightness: ScalarBinding,
    /// Additive corona layers, same mechanism and default as `Glow::layers` — only meaningful
    /// when `additive` is true (a non-additive "puff" particle never reads this field).
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
    /// One flat color, resolved once.
    Solid(ColorBinding),
    /// A hand-authored horizontal gradient: evenly spaced left-to-right color stops across the
    /// flash's own width (`2 * radius_x_px`). Any number of stops (including 1, equivalent to
    /// `Solid`) is accepted; the renderer resamples this list to its fixed internal stop count at
    /// spawn time.
    HorizontalGradient(Vec<ColorBinding>),
    /// Auto-derived from the note that triggered this flash: one flat color, sampled continuously
    /// from whichever point of that note is currently at the barrier
    /// (`render::notes::NoteInterval::color_at_barrier`) — the same mechanism
    /// `ParticleColor::MatchNote` uses, not a finer per-pixel sample of the note's actual rendered
    /// fill/sheen. Under `FlashMode::Sustained` this keeps re-evaluating for as long as the flash
    /// stays lit, so a held note's glow shifts color as more of it feeds past the barrier rather
    /// than staying pinned to its arrival color. See `docs/fmstyle-format.md`'s "Note color
    /// sampling at the barrier" section. For a genuinely multicolor flash, use
    /// `HorizontalGradient` instead.
    MatchNote,
}

impl Default for FlashColor {
    fn default() -> Self {
        FlashColor::Solid(ColorBinding::default())
    }
}

/// Volumetric "sun rays" radiating outward from a flash's center, on top of its ordinary elliptical
/// corona (`FlashSpec::layers`) — ported from `explorations/barrier-fx-lab`'s "Flash — god rays"
/// group (Phase V), aimed at a "photograph of the sun from Earth" look rather than a round blob.
/// `None` on `FlashSpec::god_rays` (default) renders the flash exactly as it rendered before this
/// phase: a plain elliptical corona, no rays.
///
/// Beams sit on `count` fixed, evenly-spaced angular slots — there is no angular wander (an earlier
/// iteration let the whole pattern drift side to side, which read as the beams wiggling rather than
/// radiating from a fixed sun, so it was removed; `rotation_speed_deg_per_sec` is kept as a rigid
/// whole-pattern spin, a different and much subtler motion). Instead each beam's own reach breathes
/// in and out over time via seeded value noise (`pulse_speed`/`pulse_amount`), on top of an
/// internal streak texture along its length (`streakiness`) and a separate whole-beam brightness
/// flicker (`flicker_speed`/`flicker_intensity`) so individual beams gutter and reappear rather than
/// staying uniformly "on". Unlike `StrandSpec`'s fixed strand-count loop, beam selection is a
/// direct per-pixel angle-to-slot computation (`render::effects`'s `god_ray_strength`/
/// `effects.wgsl`'s WGSL port of the same formula) rather than a CPU or shader loop over `count`
/// beams, so `count` has no practical upper cap.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GodRaySpec {
    /// Number of angular beam slots around the flash's center.
    #[serde(default = "default_god_ray_count")]
    pub count: u32,
    /// Beam reach in canvas px, before `length_jitter`/`pulse_amount` shrink it.
    #[serde(default = "default_god_ray_length_px")]
    pub length_px: f32,
    /// Per-beam length variation, seeded per slot so it's stable frame-to-frame: `0.0` = every beam
    /// is exactly `length_px`; `1.0` = beams range anywhere from `0` to `length_px`.
    #[serde(default = "default_god_ray_length_jitter")]
    pub length_jitter: f32,
    /// Angular falloff exponent shaping each beam's width: lower reads as wider/softer wedges,
    /// higher as narrower/sharper needles.
    #[serde(default = "default_god_ray_softness")]
    pub softness: f32,
    /// Fixed rotation of the whole beam pattern, in degrees.
    #[serde(default)]
    pub rotation_offset_deg: f32,
    /// Continuous rotation speed of the whole pattern, in degrees/second. `0.0` (default) is a
    /// no-op — see this struct's own doc comment for why angular *wander* (individual beams
    /// drifting) was rejected, as distinct from this rigid whole-pattern spin.
    #[serde(default)]
    pub rotation_speed_deg_per_sec: f32,
    /// How fast each beam's own length breathes in and out via value noise.
    #[serde(default = "default_god_ray_pulse_speed")]
    pub pulse_speed: f32,
    /// How far a beam's length can shrink at the pulse's trough, as a fraction of `length_px`
    /// (`0.0`-`1.0`).
    #[serde(default = "default_god_ray_pulse_amount")]
    pub pulse_amount: f32,
    /// Internal streak-texture contrast along each beam's length: `0.0` = a perfectly smooth beam,
    /// `1.0` = strongly streaked.
    #[serde(default = "default_god_ray_streakiness")]
    pub streakiness: f32,
    /// How fast each beam's whole-beam brightness flickers, independent of the streak texture —
    /// same value-noise mechanism as `FlashSpec::flicker_speed`, just per-beam instead of
    /// per-flash.
    #[serde(default = "default_god_ray_flicker_speed")]
    pub flicker_speed: f32,
    /// How much the flicker dims a beam at its darkest point (`0.0` never dims, `1.0` can dim a
    /// beam to fully dark at the trough).
    #[serde(default = "default_god_ray_flicker_intensity")]
    pub flicker_intensity: f32,
    /// Overall brightness multiplier on the whole god-ray contribution, independent of
    /// `FlashSpec::brightness` (which scales the ordinary corona's `layers`, not the rays).
    #[serde(default = "default_god_ray_intensity")]
    pub intensity: f32,
}

impl Default for GodRaySpec {
    fn default() -> Self {
        Self {
            count: default_god_ray_count(),
            length_px: default_god_ray_length_px(),
            length_jitter: default_god_ray_length_jitter(),
            softness: default_god_ray_softness(),
            rotation_offset_deg: 0.0,
            rotation_speed_deg_per_sec: 0.0,
            pulse_speed: default_god_ray_pulse_speed(),
            pulse_amount: default_god_ray_pulse_amount(),
            streakiness: default_god_ray_streakiness(),
            flicker_speed: default_god_ray_flicker_speed(),
            flicker_intensity: default_god_ray_flicker_intensity(),
            intensity: default_god_ray_intensity(),
        }
    }
}

fn default_god_ray_count() -> u32 {
    6
}
fn default_god_ray_length_px() -> f32 {
    420.0
}
fn default_god_ray_length_jitter() -> f32 {
    0.5
}
fn default_god_ray_softness() -> f32 {
    3.0
}
fn default_god_ray_pulse_speed() -> f32 {
    1.0
}
fn default_god_ray_pulse_amount() -> f32 {
    0.6
}
fn default_god_ray_streakiness() -> f32 {
    0.6
}
fn default_god_ray_flicker_speed() -> f32 {
    1.2
}
fn default_god_ray_flicker_intensity() -> f32 {
    0.55
}
fn default_god_ray_intensity() -> f32 {
    1.4
}

/// Faint colored ring at a fixed radius around a flash's center — a common lens-flare
/// "diffraction halo" accent, ported from the same lab exploration as `GodRaySpec` (Phase V).
/// `None` on `FlashSpec::ring` (default) renders no ring.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RingSpec {
    /// Ring radius in canvas px, measured from the flash's center.
    pub radius_px: f32,
    /// Falloff width in px around `radius_px` — smaller reads as a crisp thin ring, larger as a
    /// soft broad band.
    pub width_px: f32,
    /// Brightness multiplier; the actual on/off switch (`render::effects::ring_strength` treats
    /// `intensity <= 0.0` as fully off, same "zero is the no-op" convention `WavySpec::slide_speed`
    /// uses).
    pub intensity: f32,
}

/// Decaying radial flash spawned on note arrival. A flash is always fully opaque at spawn, fading
/// to 0 over `decay_seconds`; `brightness` alone controls how hot/white it looks, same mechanism
/// as `Glow`'s doc comment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlashSpec {
    /// Resolved per triggering note (`ScalarBinding::resolve_for_note`, same mechanism as
    /// `brightness` below) — e.g. a harder hit can spawn a bigger flash. **Breaking change**: an
    /// older `.fmstyle.ron` with a bare float on either (e.g. `radius_x_px: 40.0`) needs updating
    /// to `radius_x_px: Constant(40.0)` — see `docs/fmstyle-format.md`'s migration history.
    pub radius_x_px: ScalarBinding,
    pub radius_y_px: ScalarBinding,
    #[serde(default)]
    pub color: FlashColor,
    /// Resolved per triggering note, same mechanism as `radius_x_px`/`radius_y_px` above — e.g. a
    /// harder hit's flash can also linger longer. Same breaking-change note applies.
    pub decay_seconds: ScalarBinding,
    #[serde(default)]
    pub mode: FlashMode,
    /// Color multiplier applied at spawn time — see `Glow`'s doc comment for the overdrive
    /// mechanism; `Constant(1.0)` is a no-op (including for `FlashMode::Sustained`'s "key glow"
    /// look). Resolved once per spawned flash against the *triggering* note's velocity/pitch/track
    /// (`render::effects::spawn_flash`), same mechanism as `FlashColor::Solid`'s `ColorBinding`.
    /// **Breaking change**: an older `.fmstyle.ron` with a bare float here (e.g. `brightness: 1.0`)
    /// needs updating to `brightness: Constant(1.0)` — see `docs/fmstyle-format.md`'s migration
    /// history.
    #[serde(default)]
    pub brightness: ScalarBinding,
    /// Additive corona layers, same mechanism and default as `Glow::layers` — a flash always
    /// renders additively, so this always applies.
    #[serde(default = "default_glow_layers")]
    pub layers: [GlowLayer; 3],
    /// How fast the flash's brightness flickers over transport time, in the same value-noise-based
    /// units as `StrandSpec::flicker_speed` (not a literal Hz — bigger mutates the noise field
    /// faster). `0.0` (default) is a no-op: `render::effects::flash_flicker` degenerates to a
    /// constant, and `flicker_intensity` defaulting to `0.0` means that constant never gets read
    /// anyway. Most useful on `FlashMode::Sustained` (a long-held glow has time to visibly flicker);
    /// harmless but subtle on `Instant`, whose whole life is usually shorter than one flicker cycle.
    /// Resolved once per spawned flash against the *triggering* note's velocity/pitch/track, same
    /// mechanism as `brightness` above.
    #[serde(default = "default_zero_scalar")]
    pub flicker_speed: ScalarBinding,
    /// How much the flicker dims the flash at its darkest point: `0.0` (default) never dims at all
    /// (full brightness regardless of `flicker_speed`); `1.0` can dim all the way to fully dark at
    /// the trough. Resolved once per spawned flash, same mechanism as `flicker_speed` above.
    #[serde(default = "default_zero_scalar")]
    pub flicker_intensity: ScalarBinding,
    /// Volumetric "sun rays" radiating from the flash's center, on top of the ordinary elliptical
    /// corona above — see `GodRaySpec`'s own doc comment (Phase V). `None` (default) is a no-op:
    /// the flash renders exactly as it did before this phase.
    #[serde(default)]
    pub god_rays: Option<GodRaySpec>,
    /// A faint lens-flare-style diffraction ring around the flash's center — see `RingSpec`'s own
    /// doc comment (Phase V). `None` (default) renders no ring.
    #[serde(default)]
    pub ring: Option<RingSpec>,
    /// Lens-dispersion-style color fringing (Phase V): the whole light stack (corona + god rays +
    /// ring) is re-evaluated once per color channel at a slightly different radius from the
    /// flash's center, rather than a flat color tint — the same "error grows with distance from
    /// center" shape real lens chromatic aberration has, so the fringe shows up at the outer edge
    /// of the light, not as a uniform wash over it. `0.0` (default) is an exact no-op (single
    /// unsplit sample, pixel-identical to before this phase and half the fragment-shader cost of a
    /// non-zero value). Typical useful range is small, e.g. `0.03`-`0.1` — larger values visibly
    /// separate the channels into distinct colored ghosts rather than a subtle fringe.
    #[serde(default)]
    pub chromatic_aberration: f32,
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
    fn color_binding_by_velocity_resolves_for_note_by_interpolating_the_ramp() {
        let binding = ColorBinding::ByVelocity(Ramp {
            low: [0, 0, 0],
            high: [200, 100, 50],
        });
        assert_eq!(binding.resolve_for_note(0, 60, 0), [0, 0, 0]);
        assert_eq!(binding.resolve_for_note(127, 60, 0), [200, 100, 50]);
        assert_eq!(binding.resolve_for_note(64, 60, 0), [101, 50, 25]);
    }

    #[test]
    fn color_binding_by_pitch_class_resolves_for_note_by_pitch_modulo_12() {
        let mut colors = [[0, 0, 0]; 12];
        colors[0] = [1, 0, 0]; // C
        colors[4] = [0, 1, 0]; // E
        let binding = ColorBinding::ByPitchClass(colors);
        // MIDI note 60 is middle C; 64 is E in the same octave, 76 is E an octave up.
        assert_eq!(binding.resolve_for_note(100, 60, 0), [1, 0, 0]);
        assert_eq!(binding.resolve_for_note(100, 64, 0), [0, 1, 0]);
        assert_eq!(binding.resolve_for_note(100, 76, 0), [0, 1, 0]);
    }

    /// Unlike `ByPitchClass`, `ByPitch` scales continuously across the whole 88-key range (A0/21
    /// to C8/108) rather than repeating every octave — the lowest and highest keys resolve to the
    /// exact endpoints, and a note in between (here, the same middle C used by the `ByPitchClass`
    /// test above) lands partway between them.
    #[test]
    fn color_binding_by_pitch_resolves_for_note_across_the_whole_88_key_range() {
        let binding = ColorBinding::ByPitch(Ramp {
            low: [0, 0, 0],
            high: [255, 0, 0],
        });
        assert_eq!(binding.resolve_for_note(100, 21, 0), [0, 0, 0]);
        assert_eq!(binding.resolve_for_note(100, 108, 0), [255, 0, 0]);
        // Middle C (60) sits at (60 - 21) / (108 - 21) ≈ 0.448 of the way up.
        assert_eq!(binding.resolve_for_note(100, 60, 0), [114, 0, 0]);
        // Out-of-range pitches clamp to the nearest endpoint rather than extrapolating.
        assert_eq!(binding.resolve_for_note(100, 0, 0), [0, 0, 0]);
        assert_eq!(binding.resolve_for_note(100, 127, 0), [255, 0, 0]);
    }

    #[test]
    fn color_binding_by_track_resolves_for_note_by_track_index_with_wraparound() {
        let binding = ColorBinding::ByTrack(vec![[1, 0, 0], [0, 1, 0], [0, 0, 1]]);
        assert_eq!(binding.resolve_for_note(100, 60, 0), [1, 0, 0]);
        assert_eq!(binding.resolve_for_note(100, 60, 1), [0, 1, 0]);
        assert_eq!(binding.resolve_for_note(100, 60, 2), [0, 0, 1]);
        assert_eq!(binding.resolve_for_note(100, 60, 3), [1, 0, 0]);
        assert_eq!(
            ColorBinding::ByTrack(vec![]).resolve_for_note(100, 60, 0),
            [255, 255, 255]
        );
    }

    #[test]
    fn scalar_binding_by_velocity_resolves_for_note_by_interpolating_low_high() {
        let binding = ScalarBinding::ByVelocity {
            low: 0.0,
            high: 2.0,
        };
        assert_eq!(binding.resolve_for_note(0, 60, 0), 0.0);
        assert_eq!(binding.resolve_for_note(127, 60, 0), 2.0);
        assert_eq!(binding.resolve_for_note(64, 60, 0), 2.0 * 64.0 / 127.0);
    }

    #[test]
    fn scalar_binding_by_pitch_class_resolves_for_note_by_pitch_modulo_12() {
        let mut values = [0.0; 12];
        values[0] = 1.0; // C
        values[4] = 2.0; // E
        let binding = ScalarBinding::ByPitchClass(values);
        assert_eq!(binding.resolve_for_note(100, 60, 0), 1.0);
        assert_eq!(binding.resolve_for_note(100, 64, 0), 2.0);
        assert_eq!(binding.resolve_for_note(100, 76, 0), 2.0);
    }

    /// Same 88-key-wide scaling as `color_binding_by_pitch_resolves_for_note_across_the_whole_88_
    /// key_range`, numeric endpoints instead of colors.
    #[test]
    fn scalar_binding_by_pitch_resolves_for_note_across_the_whole_88_key_range() {
        let binding = ScalarBinding::ByPitch {
            low: 0.0,
            high: 87.0,
        };
        assert_eq!(binding.resolve_for_note(100, 21, 0), 0.0);
        assert_eq!(binding.resolve_for_note(100, 108, 0), 87.0);
        assert_eq!(binding.resolve_for_note(100, 60, 0), 39.0);
        assert_eq!(binding.resolve_for_note(100, 0, 0), 0.0);
        assert_eq!(binding.resolve_for_note(100, 127, 0), 87.0);
    }

    #[test]
    fn scalar_binding_by_track_resolves_for_note_by_track_index_with_wraparound() {
        let binding = ScalarBinding::ByTrack(vec![1.0, 2.0, 3.0]);
        assert_eq!(binding.resolve_for_note(100, 60, 0), 1.0);
        assert_eq!(binding.resolve_for_note(100, 60, 1), 2.0);
        assert_eq!(binding.resolve_for_note(100, 60, 2), 3.0);
        assert_eq!(binding.resolve_for_note(100, 60, 3), 1.0);
        assert_eq!(
            ScalarBinding::ByTrack(vec![]).resolve_for_note(100, 60, 0),
            1.0
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

    /// `NoteLayer::alpha` (a `ScalarBinding`, resolved per note like `ColorBinding`) round-trips
    /// through RON in its `ByVelocity` form, not just the `Constant` default.
    #[test]
    fn note_layer_alpha_round_trips() {
        let mut style =
            Style::from_legacy(&NoteStyle::default(), &BarrierStyle::default(), [0, 0, 0]);
        let Timed::Static(notes) = &mut style.notes else {
            unreachable!()
        };
        notes.alpha = ScalarBinding::ByVelocity {
            low: 0.2,
            high: 1.0,
        };
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    /// A `.fmstyle.ron` file predating `NoteLayer::alpha` (missing the key entirely) loads as
    /// fully opaque, not some other fallback.
    #[test]
    fn note_layer_without_alpha_field_loads_as_opaque() {
        let text = "(notes: Static((fill: Solid(Constant((1, 2, 3))), roundedness: 1.0, fall_speed: 400.0)))";
        let style: Style = ron::from_str(text).unwrap();
        let Timed::Static(notes) = &style.notes else {
            unreachable!()
        };
        assert_eq!(notes.alpha, ScalarBinding::Constant(1.0));
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

    /// A `.fmstyle.ron` file with no `background` key at all should load it as black, not an
    /// arbitrary fallback.
    #[test]
    fn style_without_background_field_loads_as_black() {
        let text = "(notes: Static((fill: Solid(Constant((1, 2, 3))), roundedness: 1.0, fall_speed: 400.0)))";
        let style: Style = ron::from_str(text).unwrap();
        assert_eq!(style.background, ColorBinding::Constant([0, 0, 0]));
    }

    /// `BarrierLayer::glow` and the `brightness` fields on `Pulse`/`FlashSpec`/`ParticleSpec`
    /// round-trip through RON.
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
                    lifetime_seconds: ScalarBinding::Constant(0.4),
                    size_px: ScalarBinding::Constant(4.0),
                    speed_px: ScalarBinding::Constant(180.0),
                    spread_degrees: ScalarBinding::Constant(60.0),
                    gravity_px: ScalarBinding::Constant(300.0),
                    color: ParticleColor::Fixed(ColorBinding::Constant([255, 240, 200])),
                    additive: true,
                    emission: EmissionMode::Burst,
                    brightness: ScalarBinding::Constant(5.0),
                    layers: default_glow_layers(),
                }),
                flash: Some(FlashSpec {
                    radius_x_px: ScalarBinding::Constant(40.0),
                    radius_y_px: ScalarBinding::Constant(40.0),
                    color: FlashColor::Solid(ColorBinding::Constant([255, 255, 255])),
                    decay_seconds: ScalarBinding::Constant(0.15),
                    mode: FlashMode::Instant,
                    brightness: ScalarBinding::Constant(6.0),
                    layers: default_glow_layers(),
                    flicker_speed: ScalarBinding::Constant(0.0),
                    flicker_intensity: ScalarBinding::Constant(0.0),
                    god_rays: None,
                    ring: None,
                    chromatic_aberration: 0.0,
                }),
            }),
            background: default_background_color(),
        };
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    /// `ParticleSpec`'s five shape scalars (`lifetime_seconds`/`size_px`/`speed_px`/
    /// `spread_degrees`/`gravity_px`) and `FlashSpec`'s five (`radius_x_px`/`radius_y_px`/
    /// `decay_seconds`/`flicker_speed`/`flicker_intensity`) round-trip in their `ByVelocity` form,
    /// not just the `Constant` shape the previous test above covers for `brightness`.
    #[test]
    fn particle_and_flash_shape_scalars_round_trip() {
        let ramp = |low: f32, high: f32| ScalarBinding::ByVelocity { low, high };
        let style = Style {
            version: 1,
            notes: Timed::default(),
            barrier: Timed::default(),
            transition: Timed::Static(TransitionLayer {
                kind: TransitionKind::ParticlesAndFlash,
                particles: Some(ParticleSpec {
                    count: 10,
                    lifetime_seconds: ramp(0.2, 0.6),
                    size_px: ramp(2.0, 5.0),
                    speed_px: ramp(100.0, 240.0),
                    spread_degrees: ramp(30.0, 90.0),
                    gravity_px: ramp(150.0, 350.0),
                    color: ParticleColor::Fixed(ColorBinding::Constant([255, 240, 200])),
                    additive: true,
                    emission: EmissionMode::Burst,
                    brightness: ScalarBinding::Constant(1.0),
                    layers: default_glow_layers(),
                }),
                flash: Some(FlashSpec {
                    radius_x_px: ramp(20.0, 50.0),
                    radius_y_px: ramp(20.0, 50.0),
                    color: FlashColor::Solid(ColorBinding::Constant([255, 255, 255])),
                    decay_seconds: ramp(0.1, 0.3),
                    mode: FlashMode::Instant,
                    brightness: ScalarBinding::Constant(1.0),
                    layers: default_glow_layers(),
                    flicker_speed: ramp(0.5, 3.0),
                    flicker_intensity: ramp(0.2, 0.8),
                    god_rays: None,
                    ring: None,
                    chromatic_aberration: 0.0,
                }),
            }),
            background: default_background_color(),
        };
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    /// A `.fmstyle.ron` file missing the `brightness`/`layers` keys on `Glow`/`Pulse`/`FlashSpec`/
    /// `ParticleSpec` should load them with the documented defaults rather than failing to parse.
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

    /// `show_bar` is a plain `#[serde(default)]` `bool`, so it defaults to `false` when a
    /// `.fmstyle.ron` doesn't set it explicitly — a style gets pure corona with no visible opaque
    /// bar unless it opts in with `show_bar: true`.
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
    /// doc comment for why `MatchNote`/`YGradient` aren't independent toggles) round-trip.
    #[test]
    fn particle_color_variants_round_trip() {
        for color in [
            ParticleColor::Fixed(ColorBinding::Constant([1, 2, 3])),
            ParticleColor::MatchNote,
            ParticleColor::YGradient {
                top: ColorBinding::Constant([10, 20, 30]),
                bottom: ColorBinding::Constant([200, 210, 220]),
                top_fraction: 0.55,
                bottom_fraction: 0.85,
            },
        ] {
            let spec = ParticleSpec {
                count: 1,
                lifetime_seconds: ScalarBinding::Constant(0.5),
                size_px: ScalarBinding::Constant(2.0),
                speed_px: ScalarBinding::Constant(100.0),
                spread_degrees: ScalarBinding::Constant(30.0),
                gravity_px: ScalarBinding::Constant(200.0),
                color: color.clone(),
                additive: true,
                emission: EmissionMode::Burst,
                brightness: ScalarBinding::Constant(1.0),
                layers: default_glow_layers(),
            };
            let text = ron::ser::to_string_pretty(&spec, ron::ser::PrettyConfig::new()).unwrap();
            let parsed: ParticleSpec = ron::from_str(&text).unwrap();
            assert_eq!(spec, parsed);
        }
    }

    #[test]
    fn y_gradient_without_fraction_fields_loads_with_defaults() {
        let text = "YGradient(top: Constant((1, 2, 3)), bottom: Constant((4, 5, 6)))";
        let color: ParticleColor = ron::from_str(text).unwrap();
        match color {
            ParticleColor::YGradient {
                top_fraction,
                bottom_fraction,
                ..
            } => {
                assert_eq!(top_fraction, 0.0);
                assert_eq!(bottom_fraction, 0.8);
            }
            _ => panic!("expected YGradient"),
        }
    }

    /// `FlashColor`'s three variants (solid, an author-painted multi-stop gradient, and the
    /// auto-derived note-at-barrier color) round-trip.
    #[test]
    fn flash_color_variants_round_trip() {
        for color in [
            FlashColor::Solid(ColorBinding::Constant([1, 2, 3])),
            FlashColor::HorizontalGradient(vec![
                ColorBinding::Constant([255, 0, 0]),
                ColorBinding::Constant([0, 255, 0]),
                ColorBinding::Constant([0, 0, 255]),
            ]),
            FlashColor::MatchNote,
        ] {
            let spec = FlashSpec {
                radius_x_px: ScalarBinding::Constant(20.0),
                radius_y_px: ScalarBinding::Constant(20.0),
                color: color.clone(),
                decay_seconds: ScalarBinding::Constant(0.2),
                mode: FlashMode::Instant,
                brightness: ScalarBinding::Constant(1.0),
                layers: default_glow_layers(),
                flicker_speed: ScalarBinding::Constant(0.0),
                flicker_intensity: ScalarBinding::Constant(0.0),
                god_rays: None,
                ring: None,
                chromatic_aberration: 0.0,
            };
            let text = ron::ser::to_string_pretty(&spec, ron::ser::PrettyConfig::new()).unwrap();
            let parsed: FlashSpec = ron::from_str(&text).unwrap();
            assert_eq!(spec, parsed);
        }
    }

    /// A `.fmstyle.ron` file missing the `god_rays`/`ring`/`chromatic_aberration` keys on
    /// `FlashSpec` (Phase V) should load them as `None`/`None`/`0.0` — the flash renders exactly
    /// as it did before this phase, same "old file still parses" contract as
    /// `flash_without_flicker_fields_loads_with_zero_default` above.
    #[test]
    fn flash_without_god_ray_fields_loads_with_no_op_defaults() {
        let text = "(radius_x_px: Constant(20.0), radius_y_px: Constant(20.0), decay_seconds: Constant(0.2))";
        let spec: FlashSpec = ron::from_str(text).unwrap();
        assert_eq!(spec.god_rays, None);
        assert_eq!(spec.ring, None);
        assert_eq!(spec.chromatic_aberration, 0.0);
    }

    /// `FlashSpec::god_rays`/`ring`/`chromatic_aberration` (Phase V) round-trip through RON.
    #[test]
    fn flash_god_rays_and_ring_round_trip() {
        let spec = FlashSpec {
            radius_x_px: ScalarBinding::Constant(11.0),
            radius_y_px: ScalarBinding::Constant(11.0),
            color: FlashColor::Solid(ColorBinding::Constant([255, 246, 224])),
            decay_seconds: ScalarBinding::Constant(0.05),
            mode: FlashMode::Instant,
            brightness: ScalarBinding::Constant(1.12),
            layers: default_glow_layers(),
            flicker_speed: ScalarBinding::Constant(0.0),
            flicker_intensity: ScalarBinding::Constant(0.0),
            god_rays: Some(GodRaySpec {
                count: 24,
                length_px: 72.0,
                length_jitter: 0.0,
                softness: 1.7,
                rotation_offset_deg: 4.0,
                rotation_speed_deg_per_sec: 0.0,
                pulse_speed: 0.0,
                pulse_amount: 0.0,
                streakiness: 1.0,
                flicker_speed: 4.16,
                flicker_intensity: 1.0,
                intensity: 0.38,
            }),
            ring: Some(RingSpec {
                radius_px: 67.0,
                width_px: 24.0,
                intensity: 0.1,
            }),
            chromatic_aberration: 0.07,
        };
        let text = ron::ser::to_string_pretty(&spec, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: FlashSpec = ron::from_str(&text).unwrap();
        assert_eq!(spec, parsed);
    }

    /// A `.fmstyle.ron` file missing the `flicker_speed`/`flicker_intensity` keys on `FlashSpec`
    /// should load them as `Constant(0.0)` (no flicker), same "old file still parses" contract as
    /// `glow_without_brightness_field_loads_with_default` above.
    #[test]
    fn flash_without_flicker_fields_loads_with_zero_default() {
        let text = "(radius_x_px: Constant(20.0), radius_y_px: Constant(20.0), decay_seconds: Constant(0.2))";
        let spec: FlashSpec = ron::from_str(text).unwrap();
        assert_eq!(spec.flicker_speed, ScalarBinding::Constant(0.0));
        assert_eq!(spec.flicker_intensity, ScalarBinding::Constant(0.0));
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
