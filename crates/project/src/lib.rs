//! RON-serializable project state: source file paths, manual sync offset, and keyboard
//! calibration. This is everything needed to reopen a project exactly where it was left.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod style;
pub use style::{
    BarrierKind, BarrierLayer, BlackKeyFill, Border, ColorBinding, EmissionMode, Fill, FlashMode,
    FlashSpec, Glow, NoteLayer, ParticleSpec, Pulse, Ramp, ScalarBinding, Sheen, Style, Timed,
    TransitionKind, TransitionLayer, WavyMode, WavySpec,
};

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Project {
    pub video_path: Option<PathBuf>,
    pub midi_path: Option<PathBuf>,
    /// `midi_time = transport_time - sync_offset_seconds`; video (and its audio) is always
    /// the master clock, this only shifts where notes render relative to it.
    pub sync_offset_seconds: f64,
    pub calibration: KeyboardCalibration,
    pub transform: VideoTransform,
    pub barrier_style: BarrierStyle,
    pub note_style: NoteStyle,
    /// A full imported `.fmstyle.ron` look, if one has been imported (see `style::Style`);
    /// `None` for a project that has never had one imported, including every project saved
    /// before this field existed (`serde(default)` makes old `.fmproj.ron` files load as
    /// `None`). When present, this is the *effective* style the renderer should use instead of
    /// one synthesized from `barrier_style`/`note_style` — see `Style::from_legacy`.
    #[serde(default)]
    pub style: Option<Style>,
}

/// Horizontal bounds of the real keyboard visible in the footage (fractions of window width,
/// 0.0 = left edge, 1.0 = right edge), plus the vertical position of the barrier where falling
/// notes stop (`barrier_fraction`, 0.0 = top of frame, 1.0 = bottom) — all fractions rather than
/// pixels so calibration survives a window resize or loading a differently-sized video.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KeyboardCalibration {
    pub left_fraction: f32,
    pub right_fraction: f32,
    pub barrier_fraction: f32,
}

impl Default for KeyboardCalibration {
    fn default() -> Self {
        Self {
            left_fraction: 0.0,
            right_fraction: 1.0,
            // Matches the hit line's position in Neothesia's own vendored waterfall shader
            // (`keyboard_y = size.y - size.y / 5.0`, i.e. always 80% down) at the time this
            // default was picked. `render::notes` now owns the shader and reads
            // `barrier_fraction` as a real uniform, so this is just a starting value, not a
            // constraint imposed by any vendored code.
            barrier_fraction: 0.8,
        }
    }
}

/// Style of the horizontal barrier where falling notes stop, drawn as a plain `egui` overlay
/// (see `ui::draw_barrier_handle`) rather than a wgpu render pass — no shader needed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BarrierStyle {
    pub color: [u8; 3],
    pub thickness: f32,
}

impl Default for BarrierStyle {
    fn default() -> Self {
        Self {
            color: [255, 255, 255],
            thickness: 4.0,
        }
    }
}

/// Style of the falling notes themselves: a single base color (sharp/black-key notes get a
/// darkened `dark` variant derived from it, one user-picked color instead of a fixed per-track
/// palette), a roundedness fraction (0.0 = square corners, 1.0 = the vendored shader's original
/// default corner radius), and `fall_speed`, the rate (pixels/second) notes travel toward the
/// barrier. `fall_speed` also scales a note's on-screen length, since `render::notes`'s shader
/// (vendored from, and still matching, Neothesia's own) sizes each note quad as
/// `duration_seconds * speed` — there is no separate "length" control.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoteStyle {
    pub color: [u8; 3],
    pub roundedness: f32,
    pub fall_speed: f32,
    /// How black-key notes are colored relative to `color` — mirrors `style::BlackKeyFill` minus
    /// gradient support (this is the legacy "quick control", not the full `.fmstyle.ron` schema).
    #[serde(default)]
    pub black_key_color: BlackKeyColorMode,
}

impl Default for NoteStyle {
    fn default() -> Self {
        Self {
            color: [93, 188, 255],
            roundedness: 1.0,
            // Matches Neothesia's own vendored default (`default_animation_speed` in
            // neothesia-core) so existing projects/behavior are unchanged until the user touches
            // the slider.
            fall_speed: 400.0,
            black_key_color: BlackKeyColorMode::default(),
        }
    }
}

/// Legacy "quick control" mirror of `style::BlackKeyFill`, minus gradient support (just a solid
/// custom color) — `Style::from_legacy` converts this into the full `BlackKeyFill` enum.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum BlackKeyColorMode {
    #[default]
    Auto,
    Same,
    Custom([u8; 3]),
}

/// Video transform applied before compositing: brightness scalar, a crop rectangle (fractions
/// of the source frame, 0.0/1.0 = uncropped), a translate (pan) offset, a full-range rotation
/// (`rotation_degrees`, -180..=180, e.g. to flip upside-down footage), and tilt (a small-angle
/// keystone/camera-correction term, not a general corner-pin). `rotation_degrees` and
/// `translate_x`/`translate_y` are affine terms; `tilt_x`/`tilt_y` are the projective (keystone)
/// terms — all folded into a single 3x3 homography matrix on the render side (see
/// `app::video_quad`), so a future true corner-pin tilt is a data change there, not a shader
/// rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VideoTransform {
    pub brightness: f32,
    pub scale: f32,
    pub rotation_degrees: f32,
    pub translate_x: f32,
    pub translate_y: f32,
    pub tilt_x: f32,
    pub tilt_y: f32,
    pub crop_left: f32,
    pub crop_right: f32,
    pub crop_top: f32,
    pub crop_bottom: f32,
}

impl Default for VideoTransform {
    fn default() -> Self {
        Self {
            brightness: 1.0,
            scale: 1.0,
            rotation_degrees: 0.0,
            translate_x: 0.0,
            translate_y: 0.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            crop_left: 0.0,
            crop_right: 1.0,
            crop_top: 0.0,
            crop_bottom: 1.0,
        }
    }
}

impl Project {
    /// The `NoteLayer` the renderer should actually draw: an imported `.fmstyle.ron`'s notes
    /// layer (resolved at `t = 0.0` — no live mid-song style swapping yet, see `Timed::resolve`),
    /// or one synthesized from the legacy `note_style`/`barrier_style` "quick controls" if no
    /// style has been imported. Shared by `app` and `export` so both consume the same effective
    /// look through one code path.
    pub fn effective_note_layer(&self) -> NoteLayer {
        self.style
            .clone()
            .unwrap_or_else(|| Style::from_legacy(&self.note_style, &self.barrier_style))
            .notes
            .resolve(0.0)
            .clone()
    }

    /// The `BarrierLayer` the renderer should actually draw — same "imported style wins,
    /// otherwise synthesize from the legacy sliders" rule as `effective_note_layer`, just for the
    /// barrier axis instead of the notes axis.
    pub fn effective_barrier_layer(&self) -> BarrierLayer {
        self.style
            .clone()
            .unwrap_or_else(|| Style::from_legacy(&self.note_style, &self.barrier_style))
            .barrier
            .resolve(0.0)
            .clone()
    }

    /// Same "imported style wins, otherwise synthesize from the legacy sliders" rule as
    /// `effective_note_layer`/`effective_barrier_layer`, for the barrier-hit transition axis.
    /// `Style::from_legacy` always produces `TransitionKind::None`, so a project with no imported
    /// style spawns no particles/flashes — matching the pre-Phase-E look exactly.
    pub fn effective_transition_layer(&self) -> TransitionLayer {
        self.style
            .clone()
            .unwrap_or_else(|| Style::from_legacy(&self.note_style, &self.barrier_style))
            .transition
            .resolve(0.0)
            .clone()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::new())
            .map_err(|err| format!("failed to serialize project: {err}"))?;
        std::fs::write(path, text).map_err(|err| format!("failed to write {path:?}: {err}"))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("failed to read {path:?}: {err}"))?;
        ron::from_str(&text).map_err(|err| format!("failed to parse {path:?}: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `.fmproj.ron` file saved before the `style` field existed has no `style` key at all;
    /// `serde(default)` should load it as `None` rather than failing to parse.
    #[test]
    fn project_without_style_field_loads_with_none() {
        let text =
            ron::ser::to_string_pretty(&Project::default(), ron::ser::PrettyConfig::new()).unwrap();
        let without_style_field: String = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("style"))
            .collect::<Vec<_>>()
            .join("\n");

        let parsed: Project = ron::from_str(&without_style_field).unwrap();
        assert_eq!(parsed.style, None);
    }
}
