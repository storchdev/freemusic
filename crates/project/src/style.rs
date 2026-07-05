//! `.fmstyle.ron` format: a data-driven description of note/barrier/transition visuals, designed
//! to be extended without breaking older files (every field is `#[serde(default)]`-compatible via
//! the wrapper types below). This module only defines the schema and its resolution helpers —
//! nothing here renders anything yet; see `CLAUDE.md` for which renderer phase consumes it.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{BarrierStyle, NoteStyle};

fn current_style_version() -> u32 {
    1
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
}

impl Default for Style {
    fn default() -> Self {
        Self {
            version: current_style_version(),
            notes: Timed::default(),
            barrier: Timed::default(),
            transition: Timed::default(),
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
    /// whatever the Keyboard tab's sliders currently hold.
    pub fn from_legacy(note_style: &NoteStyle, barrier_style: &BarrierStyle) -> Self {
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
                kind: BarrierKind::Line,
                color: ColorBinding::Constant(barrier_style.color),
                thickness: barrier_style.thickness,
                glow_radius_px: 0.0,
                pulse: None,
                wavy: None,
            }),
            transition: Timed::Static(TransitionLayer::default()),
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

/// Note fill: solid color today, a vertical gradient as the first non-solid look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Fill {
    Solid(ColorBinding),
    VerticalGradient {
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

/// Soft outer halo around a note's silhouette.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Glow {
    pub color: ColorBinding,
    pub radius_px: f32,
    pub intensity: f32,
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

/// Whether the barrier is drawn as a flat line (today's look) or a glowing bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BarrierKind {
    #[default]
    Line,
    Glow,
}

/// Barrier brightens briefly when notes arrive, then decays back to its resting intensity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pulse {
    pub intensity: f32,
    pub decay_seconds: f32,
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

/// A calm, stochastic-looking (not a single literal sine) wavy edge for the barrier — three
/// incommensurate-frequency sine terms summed with weights 0.6/0.3/0.1 (see `barrier.wgsl`'s
/// `wavy_offset`), so `|offset| <= amplitude_px` always holds exactly.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WavySpec {
    /// Peak vertical displacement in canvas pixels.
    pub amplitude_px: f32,
    /// Pixels per cycle of the slowest (dominant) term.
    pub wavelength_px: f32,
    /// How fast the wave crawls sideways over transport time; 0 = frozen shape (still
    /// x-varying), not flat.
    pub speed: f32,
    /// Which edges ripple and how. See `WavyMode`'s own doc comments.
    #[serde(default)]
    pub mode: WavyMode,
}

/// The horizontal barrier where falling notes stop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarrierLayer {
    pub kind: BarrierKind,
    pub color: ColorBinding,
    pub thickness: f32,
    pub glow_radius_px: f32,
    #[serde(default)]
    pub pulse: Option<Pulse>,
    #[serde(default)]
    pub wavy: Option<WavySpec>,
}

impl Default for BarrierLayer {
    fn default() -> Self {
        Self {
            kind: BarrierKind::default(),
            color: ColorBinding::default(),
            thickness: 4.0,
            glow_radius_px: 0.0,
            pulse: None,
            wavy: None,
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
    pub color: ColorBinding,
    pub additive: bool,
}

/// Decaying radial flash spawned on note arrival.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlashSpec {
    pub radius_x_px: f32,
    pub radius_y_px: f32,
    pub intensity: f32,
    pub color: ColorBinding,
    pub decay_seconds: f32,
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
        let style = Style::from_legacy(&NoteStyle::default(), &BarrierStyle::default());
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
    }

    #[test]
    fn black_key_fill_custom_gradient_round_trips() {
        let mut style = Style::from_legacy(&NoteStyle::default(), &BarrierStyle::default());
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

    #[test]
    fn style_default_round_trips_too() {
        let style = Style::default();
        let text = ron::ser::to_string_pretty(&style, ron::ser::PrettyConfig::new()).unwrap();
        let parsed: Style = ron::from_str(&text).unwrap();
        assert_eq!(style, parsed);
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
