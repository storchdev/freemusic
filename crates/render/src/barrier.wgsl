// Renders the barrier as a full-canvas-width horizontal bar in pixel space (top-left origin,
// y-down — same convention `notes/shader.wgsl` uses for its `@builtin(position)` distance-field
// math). No vertex buffer: six hardcoded unit-quad corners, positioned/sized entirely from
// `uniforms.geometry`/`color_glow_radius` (see `barrier.rs`'s `Uniforms` doc comment for field
// layout).

struct Uniforms {
    // x = canvas width, y = canvas height, z = barrier center y (pixels), w = thickness (pixels).
    geometry: vec4<f32>,
    // xyz = barrier color (linear), w = glow radius (pixels).
    color_glow_radius: vec4<f32>,
    // x = glow enabled (0/1), y = pulse intensity (0..1, decaying), zw unused.
    flags: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    // Local unit corners: x in [0, 1] (left -> right), y in [-1, 1] (top -> bottom of the bar).
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );

    let width = uniforms.geometry.x;
    let height = uniforms.geometry.y;
    let barrier_y = uniforms.geometry.z;
    let thickness = uniforms.geometry.w;

    // Extend the rasterized quad past the core thickness when glow is enabled, same "inflate so
    // there are pixels to paint the halo onto" trick `notes/shader.wgsl` uses for note glow — a
    // zero margin when disabled makes this an exact no-op, not just visually close.
    let glow_margin = select(0.0, uniforms.color_glow_radius.w, uniforms.flags.x > 0.5);
    let half_extent = thickness * 0.5 + glow_margin;

    let corner = corners[vertex_index];
    let pixel_x = corner.x * width;
    let pixel_y = barrier_y + corner.y * half_extent;

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
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let barrier_y = uniforms.geometry.z;
    let half_thickness = uniforms.geometry.w * 0.5;
    let vertical_dist = abs(in.position.y - barrier_y);

    let core_alpha = 1.0 - smoothstep(half_thickness - 1.0, half_thickness + 1.0, vertical_dist);

    let pulse = clamp(uniforms.flags.y, 0.0, 1.0);
    // Pulse briefly brightens the bar itself, decaying back to the base color as it settles.
    let color = mix(uniforms.color_glow_radius.rgb, vec3<f32>(1.0), pulse * 0.5);

    var alpha = core_alpha;
    if uniforms.flags.x > 0.5 {
        let glow_radius = uniforms.color_glow_radius.w;
        if glow_radius > 0.0 {
            let glow_far = half_thickness + glow_radius;
            var glow_alpha = 1.0 - smoothstep(half_thickness, glow_far, vertical_dist);
            // Always partly visible (0.35) so `BarrierKind::Glow` reads as a glow at rest, boosted
            // toward 1.0 as a note's pulse decays; scaled by `1 - core_alpha` so it only shows
            // outside/at the core's edge rather than washing out the core itself (same shape as
            // the note glow's `(1.0 - base_alpha)` scale in `notes/shader.wgsl`).
            glow_alpha = glow_alpha * (0.35 + 0.65 * pulse) * (1.0 - core_alpha);
            alpha = clamp(core_alpha + glow_alpha, 0.0, 1.0);
        }
    }

    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), alpha);
}
