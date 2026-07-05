//! Per-note instance data uploaded to the GPU. Mirrors the shape of Neothesia's own vendored
//! `NoteInstance` (`position`/`size`/`color`/`radius`) plus two fields it never had —
//! `velocity`/`track_index` — added ahead of need so `ColorBinding::ByVelocity`/`ByTrack`
//! (`project::style`) have real per-note data to read once a future phase wires them up. v1's
//! shader ignores both.

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
pub struct NoteInstance {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 3],
    pub radius: f32,
    /// Normalized MIDI velocity (0.0-1.0). Unused by the v1 shader.
    pub velocity: f32,
    /// MIDI track index as a float (vertex attributes are all floats). Unused by the v1 shader.
    pub track_index: f32,
}

impl NoteInstance {
    pub fn attributes() -> [wgpu::VertexAttribute; 6] {
        wgpu::vertex_attr_array![
            1 => Float32x2,
            2 => Float32x2,
            3 => Float32x3,
            4 => Float32,
            5 => Float32,
            6 => Float32,
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
