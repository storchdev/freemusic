// Renders 8 vertical reference lines (one per octave's C boundary) in pixel space (top-left
// origin, y-down — same convention `barrier.wgsl`/`notes/shader.wgsl` use). No vertex buffer: six
// hardcoded unit-quad corners per instance, positioned/sized entirely from `uniforms.geometry`/
// `x_positions` — one instanced draw call (`instance_index` selects which of the 8 lines) instead
// of a CPU-side loop of draw calls.

struct Uniforms {
    // x = canvas width, y = canvas height, z = barrier y (px, lines run from the top of the canvas
    // down to here), w = line width (px).
    geometry: vec4<f32>,
    // xyz = line color (linear), w = alpha (straight, not premultiplied).
    color: vec4<f32>,
    // The 8 line x positions (canvas px), packed as two vec4s.
    x_positions: array<vec4<f32>, 2>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    // Local unit corners: x in [-1, 1] (left -> right of the line's own width), y in [0, 1] (top of
    // canvas -> the barrier line).
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );

    let width = uniforms.geometry.x;
    let height = uniforms.geometry.y;
    let barrier_y = uniforms.geometry.z;
    let line_width = uniforms.geometry.w;

    let group = instance_index / 4u;
    let slot = instance_index % 4u;
    let line_x = uniforms.x_positions[group][slot];

    let corner = corners[vertex_index];
    let pixel_x = line_x + corner.x * (line_width * 0.5);
    let pixel_y = corner.y * barrier_y;

    var out: VertexOutput;
    out.position = vec4<f32>(
        pixel_x / width * 2.0 - 1.0,
        1.0 - pixel_y / height * 2.0,
        0.0,
        1.0,
    );
    return out;
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return uniforms.color;
}
