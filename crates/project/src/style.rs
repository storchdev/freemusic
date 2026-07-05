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
                color: ColorBinding::Constant(barrier_style.color),
                thickness: barrier_style.thickness,
                glow: None,
                pulse: None,
                wavy: None,
                show_bar: true,
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
/// alpha-blended ring — this is what lets it read as light radiating from a white-hot core instead
/// of a flat lighter color. `brightness` scales how much light the corona adds (and, on the
/// glowing surface's own opaque fill, still drives the pre-existing `hot_color` desaturate-toward-
/// white mix): `brightness <= 1.0` behaves as a plain dimmer, pushed past `1.0` the look reads as
/// overexposure. `brightness = 1.0` is an exact no-op. Unlike before Phase M, `brightness` no
/// longer widens how far the corona reaches — reach is purely `layers[i].sigma_px`-driven now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Glow {
    pub color: ColorBinding,
    #[serde(default = "default_brightness")]
    pub brightness: f32,
    #[serde(default = "default_glow_layers")]
    pub layers: [GlowLayer; 3],
}

impl Default for Glow {
    fn default() -> Self {
        Self {
            color: ColorBinding::default(),
            brightness: 1.0,
            layers: default_glow_layers(),
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
    pub color: ColorBinding,
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

    /// Phase K: `BarrierLayer::glow` (replacing the earlier `kind`/`glow_radius_px` pair) and the
    /// new `brightness` fields on `Pulse`/`FlashSpec`/`ParticleSpec` round-trip through RON.
    #[test]
    fn barrier_layer_with_glow_and_pulse_brightness_round_trips() {
        let mut style = Style::from_legacy(&NoteStyle::default(), &BarrierStyle::default());
        let Timed::Static(barrier) = &mut style.barrier else {
            unreachable!()
        };
        barrier.glow = Some(Glow {
            color: ColorBinding::Constant([255, 220, 120]),
            brightness: 3.0,
            layers: default_glow_layers(),
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
        let style = Style::from_legacy(&NoteStyle::default(), &BarrierStyle::default());
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
                    color: ColorBinding::Constant([255, 240, 200]),
                    additive: true,
                    emission: EmissionMode::Burst,
                    brightness: 5.0,
                    layers: default_glow_layers(),
                }),
                flash: Some(FlashSpec {
                    radius_x_px: 40.0,
                    radius_y_px: 40.0,
                    color: ColorBinding::Constant([255, 255, 255]),
                    decay_seconds: 0.15,
                    mode: FlashMode::Instant,
                    brightness: 6.0,
                    layers: default_glow_layers(),
                }),
            }),
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
    }

    #[test]
    fn pulse_without_brightness_field_loads_with_tuned_default() {
        let text = "(decay_seconds: 0.35)";
        let pulse: Pulse = ron::from_str(text).unwrap();
        assert_eq!(pulse.brightness, 1.6);
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
