//! Per-note instance data uploaded to the GPU: `position`/`size`/`color_top`/`color_bottom`/
//! `radius` plus `velocity`, `track_index`, and `canvas_gradient`. `color_bottom` lets a note
//! carry a vertical-gradient fill (`Fill::VerticalGradient`) baked in at build time rather than
//! needing a second draw call; for a solid fill it's simply equal to `color_top`.
//! `velocity`/`track_index` are unused by the shader — `ColorBinding::ByVelocity`/`ByTrack`
//! already vary fill color per note CPU-side (`notes/mod.rs::resolve_fill_for_note`), baked into
//! `color_top`/`color_bottom` before upload. `canvas_gradient` picks which span
//! `color_top`/`color_bottom` are blended across in the shader: `0.0` (default) blends across the
//! note's own local height (`Fill::Solid`/`Fill::VerticalGradient`), `1.0` blends across the
//! canvas's own Y position (`Fill::CanvasGradient`) — see `shader.wgsl`'s `fill_color` and
//! `project::Fill`'s doc comment. Baked per-note (not a style-wide uniform) since
//! `BlackKeyFill::Custom` can give sharp keys a different `Fill` variant, and therefore a
//! different gradient basis, than natural keys. `alpha` is `NoteLayer::alpha`
//! (`ScalarBinding`) resolved per note the same way as `color_top`/`color_bottom`, multiplied
//! into the note core's output alpha in `fs_core`.

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
pub struct NoteInstance {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub color_top: [f32; 3],
    pub color_bottom: [f32; 3],
    pub radius: f32,
    /// Normalized MIDI velocity (0.0-1.0). Unused by the v1 shader.
    pub velocity: f32,
    /// MIDI track index as a float (vertex attributes are all floats). Unused by the v1 shader.
    pub track_index: f32,
    /// `0.0` = blend `color_top`/`color_bottom` across the note's own local height, `1.0` = blend
    /// across the canvas's own Y position instead. See this module's doc comment.
    pub canvas_gradient: f32,
    /// Note opacity (`NoteLayer::alpha` resolved for this note), `1.0` = fully opaque. See this
    /// module's doc comment.
    pub alpha: f32,
}

impl NoteInstance {
    pub fn attributes() -> [wgpu::VertexAttribute; 9] {
        wgpu::vertex_attr_array![
            1 => Float32x2,
            2 => Float32x2,
            3 => Float32x3,
            4 => Float32x3,
            5 => Float32,
            6 => Float32,
            7 => Float32,
            8 => Float32,
            9 => Float32,
        ]
    }

    pub fn layout(attributes: &[wgpu::VertexAttribute]) -> wgpu::VertexBufferLayout<'_> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<NoteInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        }
    }
}
