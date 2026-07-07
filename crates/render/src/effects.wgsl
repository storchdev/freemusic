// Transition sprites are procedural shapes computed from each quad's local pixel offset.
// `fs_puff` handles premultiplied-alpha particles; `fs_glow` handles additive particles/flashes.
// Historical rationale for the split lives in `docs/implementation-notes.md`.

struct ViewUniform {
    transform: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> view_uniform: ViewUniform;

struct Vertex {
    @location(0) position: vec2<f32>, // unit quad, 0..1
}

struct Instance {
    @location(1) center: vec2<f32>,      // pixel-space center
    @location(2) core_radius: vec2<f32>, // configured half-extent, per axis (ellipse-aware)
    @location(3) quad_radius: vec2<f32>, // core_radius + margin for glow instances, == core_radius for puffs
    @location(4) alpha: f32,             // 0..1, already carries lifetime/decay fade
    @location(5) color: vec3<f32>,       // linear
    @location(6) layer_amp: vec3<f32>,   // additive corona layer amplitudes, brightness pre-multiplied
    @location(7) layer_sigma: vec3<f32>, // additive corona layer sigmas (px)
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) offset: vec2<f32>,      // pixel-space, center-relative
    @location(1) core_radius: vec2<f32>,
    @location(2) alpha: f32,
    @location(3) color: vec3<f32>,
    @location(4) layer_amp: vec3<f32>,
    @location(5) layer_sigma: vec3<f32>,
}

@vertex
fn vs_main(vertex: Vertex, instance: Instance) -> VertexOutput {
    let local = vertex.position * 2.0 - vec2<f32>(1.0, 1.0);
    let offset = local * instance.quad_radius;
    let pixel = instance.center + offset;

    var out: VertexOutput;
    out.position = view_uniform.transform * vec4<f32>(pixel, 0.0, 1.0);
    out.offset = offset;
    out.core_radius = instance.core_radius;
    out.alpha = instance.alpha;
    out.color = instance.color;
    out.layer_amp = instance.layer_amp;
    out.layer_sigma = instance.layer_sigma;
    return out;
}

// Hard-edged dot (today's non-additive particle look, unchanged): solid core out to 60% of the
// radius, fading to fully transparent at the edge. `offset / core_radius` reduces to exactly
// `local` when `quad_radius == core_radius` (always true for puffs), so this is pixel-identical to
// the pre-Phase-M `length(in.local)` formula.
@fragment
fn fs_puff(in: VertexOutput) -> @location(0) vec4<f32> {
    let d = length(in.offset / in.core_radius);
    let hard_edge = 1.0 - smoothstep(0.6, 1.0, d);
    let a = clamp(in.alpha, 0.0, 1.0) * hard_edge;
    return vec4<f32>(in.color * a, a);
}

// Additive corona (Phase M): sums three exponential falloff terms
// (`amplitude * exp(-edge_dist_px / sigma_px)`) into a single light value, added onto whatever is
// already in the framebuffer (`ONE`/`ONE` blend, see `effects.rs`) — see `barrier.wgsl`'s
// `fs_glow` for the full rationale. `edge_dist_px` is an ellipse-aware distance in pixels outside
// the instance's `core_radius` (0 inside it). No separate opaque core is drawn here (unlike
// barrier/notes) — additive light never needs to occlude anything, so a bright center is just
// where the tight/near-field layer dominates, not a distinct pipeline.
@fragment
fn fs_glow(in: VertexOutput) -> @location(0) vec4<f32> {
    let norm = length(in.offset / in.core_radius);
    let edge_dist_px = max(norm - 1.0, 0.0) * min(in.core_radius.x, in.core_radius.y);

    var strength = 0.0;
    strength += in.layer_amp.x * exp(-edge_dist_px / max(in.layer_sigma.x, 0.01));
    strength += in.layer_amp.y * exp(-edge_dist_px / max(in.layer_sigma.y, 0.01));
    strength += in.layer_amp.z * exp(-edge_dist_px / max(in.layer_sigma.z, 0.01));

    let light = in.color * strength * clamp(in.alpha, 0.0, 1.0);
    return vec4<f32>(light, 1.0);
}
