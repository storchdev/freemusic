//! RON-serializable project state: source file paths, manual sync offset, and keyboard
//! calibration. This is everything needed to reopen a project exactly where it was left.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
            // Matches the hit line's hardcoded position in Neothesia's own vendored waterfall
            // shader (`keyboard_y = size.y - size.y / 5.0`, i.e. always 80% down) — see
            // `render::midi_overlay`'s barrier-viewport trick for how an arbitrary
            // `barrier_fraction` is actually achieved without forking that shader.
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
/// darkened `dark` variant derived from it, same idea as Neothesia's own per-track
/// `ColorSchemaV1` but with one user-picked color instead of a fixed per-track palette), a
/// roundedness fraction (0.0 = square corners, 1.0 = Neothesia's own default corner radius), and
/// `fall_speed`, the rate (pixels/second) notes travel toward the barrier. `fall_speed` also
/// scales a note's on-screen length, since Neothesia's vendored waterfall shader sizes each note
/// quad as `duration_seconds * speed` — there is no separate "length" control.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoteStyle {
    pub color: [u8; 3],
    pub roundedness: f32,
    pub fall_speed: f32,
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
        }
    }
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
