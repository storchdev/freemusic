// Renders the transition pass: a fixed pool of particles plus decaying flashes spawned when a
// note arrives at the barrier (Phase E of the `.fmstyle.ron` milestone). No external texture asset
// — every sprite is a procedural soft radial circle computed in the fragment shader from the
// quad's local (-1..1) coordinate, same "signed distance in the fragment shader" spirit as
// `notes/shader.wgsl`'s rounded-rect and `barrier.wgsl`'s glow falloff.
//
// One instanced draw per blend mode (see `effects.rs`): additive (flashes, and particles when
// `ParticleSpec::additive` is true) and premultiplied-alpha (particles when `additive` is false).
// The fragment shader always outputs premultiplied color (`rgb * alpha`, `alpha`) so both
// pipelines can share the exact same shader and only differ in `BlendState`.

struct ViewUniform {
    transform: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> view_uniform: ViewUniform;

struct Vertex {
    @location(0) position: vec2<f32>, // unit quad, 0..1
}

struct Instance {
    @location(1) center: vec2<f32>, // pixel-space center
    @location(2) radius: vec2<f32>, // pixel-space half-extent, per axis (ellipse when x != y)
    @location(3) alpha: f32,        // 0..1, already carries lifetime/decay fade
    @location(4) color: vec3<f32>,  // linear
    @location(5) softness: f32,     // 0.0 = hard-edged dot (particles), 1.0 = radiating glow (flashes)
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>, // -1..1, center-relative (pre-scale unit-quad coordinate)
    @location(1) alpha: f32,
    @location(2) color: vec3<f32>,
    @location(3) softness: f32,
}

@vertex
fn vs_main(vertex: Vertex, instance: Instance) -> VertexOutput {
    let local = vertex.position * 2.0 - vec2<f32>(1.0, 1.0);
    let pixel = instance.center + local * instance.radius;

    var out: VertexOutput;
    out.position = view_uniform.transform * vec4<f32>(pixel, 0.0, 1.0);
    out.local = local;
    out.alpha = instance.alpha;
    out.color = instance.color;
    out.softness = instance.softness;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // `pixel = center + local * radius` is affine in `local`, so the interpolated `in.local` at
    // any fragment already equals `(pixel - center) / radius` component-wise — `length(in.local)`
    // is therefore already the correct elliptical-normalized radius when radius.x != radius.y, no
    // extra per-axis handling needed here.
    let d = length(in.local);
    // Hard-edged dot (today's particle look, unchanged): solid core out to 60% of the radius,
    // fading to fully transparent at the edge.
    let hard_edge = 1.0 - smoothstep(0.6, 1.0, d);
    // Soft radiating glow (flashes): bright core fading smoothly across the whole radius rather
    // than staying solid until a thin outer band.
    let soft_glow = pow(clamp(1.0 - d, 0.0, 1.0), 1.6);
    let shape = mix(hard_edge, soft_glow, in.softness);
    let a = clamp(in.alpha, 0.0, 1.0) * shape;
    return vec4<f32>(in.color * a, a);
}
