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
}

/// Horizontal bounds of the real keyboard visible in the footage, as fractions of window
/// width (0.0 = left edge, 1.0 = right edge). Fractions rather than pixels so calibration
/// survives a window resize or loading a differently-sized video.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KeyboardCalibration {
    pub left_fraction: f32,
    pub right_fraction: f32,
}

impl Default for KeyboardCalibration {
    fn default() -> Self {
        Self {
            left_fraction: 0.0,
            right_fraction: 1.0,
        }
    }
}

/// Video transform applied before compositing: brightness scalar, a crop rectangle (fractions
/// of the source frame, 0.0/1.0 = uncropped), a translate (pan) offset, and rotation/tilt
/// (small-angle camera-correction terms, not a general corner-pin). `rotation_degrees` and
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
