// Transition sprites are procedural shapes computed from each quad's local pixel offset.
// `fs_puff` handles premultiplied-alpha particles; `fs_glow` handles additive particles/flashes.
// Historical rationale for the split lives in `docs/implementation-notes.md`.
//
// `color_stops` (5 of them, `NOTE_COLOR_STOPS` on the Rust side) replaced a single `color`: a
// flash can carry a horizontal gradient (author-painted or sampled from the note that triggered
// it — see `project::FlashColor`), so every instance now carries 5 evenly-spaced left-to-right
// stops instead of one flat color. A particle (which only ever has one color) simply has every
// stop baked equal at spawn time, so interpolating across them is a no-op and reproduces the old
// single-color look exactly.

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
    @location(5) color_stop_0: vec3<f32>,
    @location(6) color_stop_1: vec3<f32>,
    @location(7) color_stop_2: vec3<f32>,
    @location(8) color_stop_3: vec3<f32>,
    @location(9) color_stop_4: vec3<f32>,
    @location(10) layer_amp: vec3<f32>,   // additive corona layer amplitudes, brightness pre-multiplied
    @location(11) layer_sigma: vec3<f32>, // additive corona layer sigmas (px)
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) offset: vec2<f32>,      // pixel-space, center-relative
    @location(1) core_radius: vec2<f32>,
    @location(2) alpha: f32,
    @location(3) color_stop_0: vec3<f32>,
    @location(4) color_stop_1: vec3<f32>,
    @location(5) color_stop_2: vec3<f32>,
    @location(6) color_stop_3: vec3<f32>,
    @location(7) color_stop_4: vec3<f32>,
    @location(8) layer_amp: vec3<f32>,
    @location(9) layer_sigma: vec3<f32>,
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
    out.color_stop_0 = instance.color_stop_0;
    out.color_stop_1 = instance.color_stop_1;
    out.color_stop_2 = instance.color_stop_2;
    out.color_stop_3 = instance.color_stop_3;
    out.color_stop_4 = instance.color_stop_4;
    out.layer_amp = instance.layer_amp;
    out.layer_sigma = instance.layer_sigma;
    return out;
}

// Interpolates the 5 color stops at horizontal fraction `t` (0 = instance's own left edge, 1 =
// its right edge) — mirrors `render::effects::sample_color_stops`'s CPU-side math exactly.
fn sample_stops(in: VertexOutput, t: f32) -> vec3<f32> {
    let tc = clamp(t, 0.0, 1.0);
    let scaled = tc * 4.0;
    let i0 = u32(floor(scaled));
    let frac = scaled - f32(i0);
    if i0 >= 4u {
        return in.color_stop_4;
    } else if i0 == 3u {
        return mix(in.color_stop_3, in.color_stop_4, frac);
    } else if i0 == 2u {
        return mix(in.color_stop_2, in.color_stop_3, frac);
    } else if i0 == 1u {
        return mix(in.color_stop_1, in.color_stop_2, frac);
    } else {
        return mix(in.color_stop_0, in.color_stop_1, frac);
    }
}

// Instance-local horizontal fraction (0 at the instance's own left edge, 1 at its right edge),
// shared by both fragment entry points below.
fn horizontal_fraction(in: VertexOutput) -> f32 {
    return clamp(in.offset.x / (2.0 * in.core_radius.x) + 0.5, 0.0, 1.0);
}

// Hard-edged dot (today's non-additive particle look, unchanged): solid core out to 60% of the
// radius, fading to fully transparent at the edge. `offset / core_radius` reduces to exactly
// `local` when `quad_radius == core_radius` (always true for puffs), so this is pixel-identical to
// the pre-Phase-M `length(in.local)` formula.
@fragment
fn fs_puff(in: VertexOutput) -> @location(0) vec4<f32> {
    let d = length(in.offset / in.core_radius);
    let hard_edge = 1.0 - smoothstep(0.6, 1.0, d);
    let color = sample_stops(in, horizontal_fraction(in));
    let a = clamp(in.alpha, 0.0, 1.0) * hard_edge;
    return vec4<f32>(color * a, a);
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

    let color = sample_stops(in, horizontal_fraction(in));
    let light = color * strength * clamp(in.alpha, 0.0, 1.0);
    return vec4<f32>(light, 1.0);
}
