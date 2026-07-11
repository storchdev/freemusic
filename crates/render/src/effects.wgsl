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
    // x = transport time (seconds), used only by the god-ray pulse/flicker/rotation noise below —
    // yzw unused, packed into a vec4 rather than a bare trailing f32 to match this codebase's
    // uniform-buffer convention (see barrier.wgsl's `Uniforms`) and avoid any manual tail padding.
    time: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> view_uniform: ViewUniform;

struct Vertex {
    @location(0) position: vec2<f32>, // unit quad, 0..1
}

// Phase V: `godray_a`/`godray_b`/`godray_c`/`ring_chromatic` add the "photograph of the sun from
// Earth" flash extras (volumetric god rays, a diffraction ring, chromatic aberration) ported from
// `explorations/barrier-fx-lab` — see `project::GodRaySpec`/`RingSpec`/`FlashSpec::
// chromatic_aberration` for what each packed field means. Puff/particle instances (and any flash
// with `god_rays`/`ring: None`, `chromatic_aberration: 0.0`) simply carry these zeroed
// (`godray_a.x == 0.0` count and `ring_chromatic.z`/`.w == 0.0` both gate their own effect off in
// `fs_glow`), so this is a pixel-identical no-op for every instance that predates this phase.
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
    @location(12) godray_a: vec4<f32>,    // x = count, y = length_px, z = length_jitter, w = softness
    @location(13) godray_b: vec4<f32>,    // x = rotation_offset_deg, y = rotation_speed_deg_per_sec, z = pulse_speed, w = pulse_amount
    @location(14) godray_c: vec4<f32>,    // x = streakiness, y = flicker_speed, z = flicker_intensity, w = intensity
    @location(15) ring_chromatic: vec4<f32>, // x = ring_radius_px, y = ring_width_px, z = ring_intensity, w = chromatic_aberration
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
    @location(10) godray_a: vec4<f32>,
    @location(11) godray_b: vec4<f32>,
    @location(12) godray_c: vec4<f32>,
    @location(13) ring_chromatic: vec4<f32>,
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
    out.godray_a = instance.godray_a;
    out.godray_b = instance.godray_b;
    out.godray_c = instance.godray_c;
    out.ring_chromatic = instance.ring_chromatic;
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
// (`amplitude * exp(-edge_dist_px / sigma_px)`) into a single light value — see `barrier.wgsl`'s
// `fs_glow` for the full rationale. `edge_dist_px` is an ellipse-aware distance in pixels outside
// the instance's `core_radius` (0 inside it). No separate opaque core is drawn here (unlike
// barrier/notes) — additive light never needs to occlude anything, so a bright center is just
// where the tight/near-field layer dominates, not a distinct pipeline.
fn core_strength(offset: vec2<f32>, core_radius: vec2<f32>, layer_amp: vec3<f32>, layer_sigma: vec3<f32>) -> f32 {
    let norm = length(offset / core_radius);
    let edge_dist_px = max(norm - 1.0, 0.0) * min(core_radius.x, core_radius.y);

    var strength = 0.0;
    strength += layer_amp.x * exp(-edge_dist_px / max(layer_sigma.x, 0.01));
    strength += layer_amp.y * exp(-edge_dist_px / max(layer_sigma.y, 0.01));
    strength += layer_amp.z * exp(-edge_dist_px / max(layer_sigma.z, 0.01));
    return strength;
}

// Phase V: god rays / halo ring / chromatic aberration, ported from
// `explorations/barrier-fx-lab/barrier-fx-lab.html`'s "Flash — god rays"/"halo"/"chromatic
// aberration" groups (`flashGodRayStrength`/`flashRingStrength`/`flashContribution`), aimed at a
// "photograph of the sun from Earth" look rather than a round blob. Same value-noise construction
// (`hash21`/`noise2`) as `barrier.wgsl`'s strand-bundle flicker.

const TWO_PI: f32 = 6.28318530718;
// "The target look settled on a fixed shape for these two -- they're not exposed as sliders, just
// baked in here" (same rationale as the lab's own comment on these constants). GOD_RAY_TAPER
// shapes the brightness gradient along a beam's length; GOD_RAY_NOISE_SCALE sizes the streak
// texture sampled along each beam.
const GOD_RAY_TAPER: f32 = 0.65;
const GOD_RAY_NOISE_SCALE: f32 = 0.2;

fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * vec3<f32>(0.1031, 0.1030, 0.0973));
    p3 += vec3<f32>(dot(p3, p3.yzx + 33.33));
    return fract((p3.x + p3.y) * p3.z);
}

fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 2.0 - 1.0;
}

// WGSL's `%` follows the dividend's sign (truncated division); the lab's GLSL `mod()` follows the
// divisor's sign (floored division) — this matters here since `theta - rot` can be negative.
fn floor_mod(x: f32, y: f32) -> f32 {
    return x - y * floor(x / y);
}

// A handful of much *wider* and *longer* beams than a typical starburst (low `softness` exponent =
// broad angular cone), sitting on `count` fixed, evenly-spaced angular slots — no angular wander
// (that read as the beams wiggling side to side, not the intended look). Each beam's own reach
// breathes in and out over time via noise (`pulse_speed`/`pulse_amount`), plus a whole-beam
// brightness flicker on top of an internal streak texture. Unlike the strand bundle, this is a
// direct per-pixel angle-to-slot computation, not a loop over `count` beams — `count` has no
// practical upper cap. `params_a` = (count, length_px, length_jitter, softness), `params_b` =
// (rotation_offset_deg, rotation_speed_deg_per_sec, pulse_speed, pulse_amount), `params_c` =
// (streakiness, flicker_speed, flicker_intensity, intensity) — see `project::GodRaySpec`'s own
// field docs for what each means. `count < 0.5` (the zeroed-instance default) is the off switch.
fn god_ray_strength(offset: vec2<f32>, core_radius: vec2<f32>, params_a: vec4<f32>, params_b: vec4<f32>, params_c: vec4<f32>, time_seconds: f32) -> f32 {
    let count = params_a.x;
    if (count < 0.5) {
        return 0.0;
    }
    let length_px = params_a.y;
    let length_jitter = params_a.z;
    let softness = params_a.w;
    let rotation_offset_deg = params_b.x;
    let rotation_speed = params_b.y;
    let pulse_speed = params_b.z;
    let pulse_amount = params_b.w;
    let streakiness = params_c.x;
    let flicker_speed = params_c.y;
    let flicker_intensity = params_c.z;
    let intensity = params_c.w;

    let r = length(offset);
    let theta = atan2(offset.y, offset.x);
    let rot = radians(rotation_offset_deg + time_seconds * rotation_speed);
    let theta_r = floor_mod(theta - rot, TWO_PI);
    let slot = theta_r / TWO_PI * count;
    let idx = floor(slot);
    let frac_slot = fract(slot) - 0.5;

    let seed = hash21(vec2<f32>(idx * 31.7, 11.3));

    // Each beam's own length is modulated by its own noise-driven pulse (not a fixed value), so
    // the beam visibly grows and shrinks rather than staying a static wedge -- `seed` offsets each
    // beam's phase so they don't all breathe in lockstep.
    let pulse_n = clamp(noise2(vec2<f32>(seed * 23.0 + 4.0, time_seconds * pulse_speed + seed * 7.0)) * 0.5 + 0.5, 0.0, 1.0);
    let len_pulse = mix(1.0 - pulse_amount, 1.0, pulse_n);
    let len = length_px * (1.0 - length_jitter * 0.5 + length_jitter * seed) * len_pulse;

    let ang_fall = pow(max(cos(frac_slot * 3.14159265), 0.0), max(softness, 0.1));
    // `rad_fall` alone shapes the brightness gradient along the beam, but for `GOD_RAY_TAPER < 1.0`
    // its power-law tail never really reaches zero -- `outer_cut` is a hard boundary tied directly
    // to `len` so beam length (and its pulse) actually confines where the ray is visible.
    let rad_fall = exp(-pow(r / max(len, 1.0), GOD_RAY_TAPER));
    let outer_cut = 1.0 - smoothstep(len * 0.7, len * 1.15, r);

    let streak_n = clamp(noise2(vec2<f32>(idx * 5.0 + seed * 9.0, r * GOD_RAY_NOISE_SCALE * 0.02)) * 0.5 + 0.5, 0.0, 1.0);
    let streak = mix(1.0 - streakiness, 1.0, streak_n);

    let flick = pow(clamp(noise2(vec2<f32>(idx * 4.1 + 6.0, time_seconds * flicker_speed + seed * 19.0)) * 0.5 + 0.5, 0.0, 1.0), 2.2);
    let beam_flick = 1.0 - flicker_intensity + flicker_intensity * flick;

    let inner_cut = smoothstep(0.0, min(core_radius.x, core_radius.y) * 0.4, r);

    return ang_fall * rad_fall * streak * inner_cut * outer_cut * beam_flick * intensity;
}

// Faint colored ring at a fixed radius -- a common lens-flare "diffraction halo" accent.
// `ring_intensity <= 0.0` (the zeroed-instance default) is the off switch.
fn ring_strength(offset: vec2<f32>, ring_radius: f32, ring_width: f32, ring_intensity: f32) -> f32 {
    if (ring_intensity <= 0.0) {
        return 0.0;
    }
    let d = abs(length(offset) - ring_radius);
    return exp(-d / max(ring_width, 0.1)) * ring_intensity;
}

// The combined (colorless) light strength at `offset` -- shared by every channel sample
// `fs_glow` takes below when chromatic aberration is enabled.
fn total_strength(in: VertexOutput, offset: vec2<f32>, time_seconds: f32) -> f32 {
    var s = core_strength(offset, in.core_radius, in.layer_amp, in.layer_sigma);
    s += god_ray_strength(offset, in.core_radius, in.godray_a, in.godray_b, in.godray_c, time_seconds);
    s += ring_strength(offset, in.ring_chromatic.x, in.ring_chromatic.y, in.ring_chromatic.z);
    return s;
}

@fragment
fn fs_glow(in: VertexOutput) -> @location(0) vec4<f32> {
    let time_seconds = view_uniform.time.x;
    let chromatic_amount = in.ring_chromatic.w;

    var strength_rgb: vec3<f32>;
    if (chromatic_amount > 0.0) {
        // Rather than the usual "sample a texture three times" trick (there's no texture here,
        // everything is procedural), each color channel re-evaluates the *entire* light stack
        // with `offset` scaled by a slightly different factor -- exactly like a lens's radial
        // distortion varying by wavelength, the error is ~0 near the flash center and grows with
        // distance from it.
        let ca = chromatic_amount;
        strength_rgb = vec3<f32>(
            total_strength(in, in.offset * (1.0 + ca), time_seconds),
            total_strength(in, in.offset, time_seconds),
            total_strength(in, in.offset * (1.0 - ca), time_seconds),
        );
    } else {
        let s = total_strength(in, in.offset, time_seconds);
        strength_rgb = vec3<f32>(s, s, s);
    }

    let color = sample_stops(in, horizontal_fraction(in));
    let light = color * strength_rgb * clamp(in.alpha, 0.0, 1.0);
    return vec4<f32>(light, 1.0);
}
