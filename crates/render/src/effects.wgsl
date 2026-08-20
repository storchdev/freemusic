// Transition sprites are procedural shapes computed from each quad's local pixel offset.
// `fs_puff` handles premultiplied-alpha particles; `fs_glow` handles additive particles/flashes.
// Historical rationale for the split lives in `docs/narratives/architecture.md`.
//
// `color_stops` (5 of them, `NOTE_COLOR_STOPS` on the Rust side) replaced a single `color`: a
// flash can carry a horizontal gradient (author-painted or sampled from the note that triggered
// it — see `project::FlashColor`), so every instance now carries 5 evenly-spaced left-to-right
// stops instead of one flat color. A particle (which only ever has one color) simply has every
// stop baked equal at spawn time, so interpolating across them is a no-op and reproduces the old
// single-color look exactly.

struct ViewUniform {
    transform: mat4x4<f32>,
    // x = transport time (seconds), used only by the flame-corona silhouette/streak/flicker noise
    // below — yzw unused, packed into a vec4 rather than a bare trailing f32 to match this
    // codebase's uniform-buffer convention (see barrier.wgsl's `Uniforms`) and avoid any manual
    // tail padding.
    time: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> view_uniform: ViewUniform;

struct Vertex {
    @location(0) position: vec2<f32>, // unit quad, 0..1
}

// `flame_a`/`flame_b`/`flame_c`/`ring_chromatic` carry the flash extras (a 360-degree flame
// corona, a diffraction ring, chromatic aberration) ported from `explorations/barrier-fx-lab` --
// see `project::FlameCoronaSpec`/`RingSpec`/`FlashSpec::chromatic_aberration` for what each packed
// field means. Puff/particle instances (and any flash with `flame_corona`/`ring: None`,
// `chromatic_aberration: 0.0`) simply carry these zeroed (`flame_c.y == 0.0` intensity and
// `ring_chromatic.z`/`.w == 0.0` both gate their own effect off in `fs_glow`), so this is a
// pixel-identical no-op for every instance that doesn't use them.
// `project::FlashSpec::turbulence` follows the same convention: an unset `TurbulenceSpec` leaves
// its packed slots (`core_radius.zw`, `layer_amp.w` -- see this struct's own doc comment on why
// they're packed there rather than a dedicated field/location) zeroed, and `strength_px <= 0.0` is
// `domain_warp`'s own off switch.
// wgpu's vertex-attribute-location limit is 16 (indices 0..15 across *all* buffers bound to one
// pipeline, including `Vertex`'s own @location(0)) -- this `Instance` struct is already at exactly
// that limit, so `core_radius`/`layer_amp`/`layer_sigma` below are each widened from vec2/vec3 to
// vec4 to steal their otherwise-unused trailing component(s) instead of costing extra locations --
// `layer_sigma.w` in particular carries `FlameCoronaSpec::flicker_independence`, the thirteenth
// flame-corona field that doesn't fit in `flame_a`/`b`/`c`'s twelve slots. `vs_main` unpacks these
// into `VertexOutput`'s own (unconstrained -- inter-stage varyings have a much higher limit)
// `core_radius`/`layer_amp`/`layer_sigma`/`turbulence`/`flicker_independence` fields.
struct Instance {
    @location(1) center: vec2<f32>,      // pixel-space center
    @location(2) core_radius: vec4<f32>, // xy = configured half-extent (ellipse-aware); z = turbulence strength_px; w = turbulence scale_px
    @location(3) quad_radius: vec2<f32>, // core_radius.xy + margin for glow instances, == core_radius.xy for puffs
    @location(4) alpha: f32,             // 0..1, already carries lifetime/decay fade
    @location(5) color_stop_0: vec3<f32>,
    @location(6) color_stop_1: vec3<f32>,
    @location(7) color_stop_2: vec3<f32>,
    @location(8) color_stop_3: vec3<f32>,
    @location(9) color_stop_4: vec3<f32>,
    @location(10) layer_amp: vec4<f32>,   // xyz = additive corona layer amplitudes, brightness pre-multiplied; w = turbulence speed
    @location(11) layer_sigma: vec4<f32>, // xyz = additive corona layer sigmas (px); w = flame corona flicker_independence
    @location(12) flame_a: vec4<f32>,     // x = lobes, y = reach_variance, z = silhouette_speed, w = base_reach_px
    @location(13) flame_b: vec4<f32>,     // x = streak_freq, y = streak_scale_px, z = streakiness, w = core_frac
    @location(14) flame_c: vec4<f32>,     // x = tip_softness_px, y = intensity, z = flicker_speed, w = flicker_intensity
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
    @location(10) flame_a: vec4<f32>,
    @location(11) flame_b: vec4<f32>,
    @location(12) flame_c: vec4<f32>,
    @location(13) ring_chromatic: vec4<f32>,
    @location(14) turbulence: vec4<f32>,
    @location(15) flicker_independence: f32,
}

@vertex
fn vs_main(vertex: Vertex, instance: Instance) -> VertexOutput {
    let local = vertex.position * 2.0 - vec2<f32>(1.0, 1.0);
    let offset = local * instance.quad_radius;
    let pixel = instance.center + offset;

    var out: VertexOutput;
    out.position = view_uniform.transform * vec4<f32>(pixel, 0.0, 1.0);
    out.offset = offset;
    out.core_radius = instance.core_radius.xy;
    out.alpha = instance.alpha;
    out.color_stop_0 = instance.color_stop_0;
    out.color_stop_1 = instance.color_stop_1;
    out.color_stop_2 = instance.color_stop_2;
    out.color_stop_3 = instance.color_stop_3;
    out.color_stop_4 = instance.color_stop_4;
    out.layer_amp = instance.layer_amp.xyz;
    out.layer_sigma = instance.layer_sigma.xyz;
    // `core_radius.zw` = turbulence (strength_px, scale_px), `layer_amp.w` = turbulence speed --
    // see `Instance`'s own doc comment for why these live packed here instead of a dedicated field.
    out.turbulence = vec4<f32>(instance.core_radius.z, instance.core_radius.w, instance.layer_amp.w, 0.0);
    out.flicker_independence = instance.layer_sigma.w;
    out.flame_a = instance.flame_a;
    out.flame_b = instance.flame_b;
    out.flame_c = instance.flame_c;
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
// `fs_glow` for the full rationale. `edge_dist_px` is an ellipse-aware *signed* distance in pixels
// from the instance's `core_radius` boundary (negative inside it, positive outside). No separate
// opaque core is drawn here (unlike barrier/notes) — additive light never needs to occlude
// anything, so a bright center is just where the tight/near-field layer dominates, not a distinct
// pipeline.
//
// Unconditionally signed rather than clamped to 0 inside the boundary (an earlier version did
// `select(0.0, edge_dist_px, norm > 1.0)`): clamping made every pixel inside `core_radius` sum to
// the exact same flat plateau (the three layer amplitudes with zero falloff applied), so the
// corona read as a hard-edged solid disc with a visible slope discontinuity right at the
// boundary -- a real point light has no such plateau, its brightness rises continuously all the
// way to the center. Removing the clamp lets the same exponential curve that shapes the outer
// halo continue inward, peaking at the center instead of flattening out.
fn core_strength(offset: vec2<f32>, core_radius: vec2<f32>, layer_amp: vec3<f32>, layer_sigma: vec3<f32>) -> f32 {
    let norm = length(offset / core_radius);
    // `offset / norm` is the point where the ray from the center through `offset` crosses the
    // ellipse boundary (exact on both axes, a close approximation elsewhere), so
    // `length(offset) - length(offset) / norm` is the real signed pixel distance from that
    // boundary to `offset` (negative when `offset` sits inside the ellipse, since `1.0 / norm` is
    // then > 1). Rescaling `(norm - 1.0)` by `min(core_radius.x, core_radius.y)` (an even older
    // formula) badly underestimates this away from the minor axis for an elongated ellipse (e.g. a
    // flash's wide, flat corona) -- the falloff then decays far slower in real pixels than
    // `sigma_px` intends and outruns the quad margin sized from it (`spawn_flash`'s `margin_px`),
    // producing a hard rectangular clip at the quad edge instead of a soft fade to zero.
    let dist = length(offset);
    // `max(norm, 0.0001)` keeps `1.0 / norm` finite at `offset == vec2(0.0)` (`norm == 0.0`) --
    // `dist` is also exactly 0 there, so the product stays a well-defined 0 rather than a NaN from
    // `0 * inf`.
    let edge_dist_px = dist * (1.0 - 1.0 / max(norm, 0.0001));

    var strength = 0.0;
    strength += layer_amp.x * exp(-edge_dist_px / max(layer_sigma.x, 0.01));
    strength += layer_amp.y * exp(-edge_dist_px / max(layer_sigma.y, 0.01));
    strength += layer_amp.z * exp(-edge_dist_px / max(layer_sigma.z, 0.01));
    return strength;
}

// Flame corona / halo ring / chromatic aberration, ported from
// `explorations/barrier-fx-lab/barrier-fx-lab.html`'s "Flash — flame corona"/"halo"/"chromatic
// aberration" groups (`flameCoronaStrength`/`flashRingStrength`/`flashContribution`), aimed at an
// aurora-curtain/solar-corona-photograph look rather than a round blob (this replaced an earlier
// beam-based "god ray" design entirely — see `docs/narratives/fmstyle-history.md`'s
// breaking-change log). Same value-noise construction (`hash21`/`noise2`) as `barrier.wgsl`'s
// strand-bundle flicker.

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

// 4-octave fractal noise built on `noise2` -- used by `flame_corona_strength` below for both the
// silhouette (how far the flame reaches at each angle) and the internal streak texture, so each
// gets organic multi-scale detail rather than a single smooth sine/noise lobe.
fn fbm(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    var sum = 0.0;
    var amp = 0.5;
    for (var i = 0; i < 4; i = i + 1) {
        sum += amp * noise2(p);
        p *= 2.03;
        amp *= 0.5;
    }
    return sum;
}

// A point on a circle of radius `freq` in noise-space, parametrized by the light's own unit
// direction vector (`dir = offset / r`) rather than by `atan2`'s angle. Sweeping `dir` all the way
// around traces the full circumference (`2*PI*freq` noise-space units), giving roughly the same
// bump density per full turn a raw `theta * freq` noise coordinate would -- but since `dir` is
// just `offset / r`, a continuous function of position with no branch, there's no seam where
// `atan2` wraps from +PI to -PI the way a `theta * freq` coordinate would (a hard discontinuity
// fixed at "pointing left" no matter how the frequency is tuned).
fn angular_noise_point(dir: vec2<f32>, freq: f32) -> vec2<f32> {
    return dir * freq;
}

// Continuous flame corona wrapping the full 360 degrees around the light center. The raggedness
// *is* the shape, not an overlay: `reach` (how far the flame extends at each angle) is itself a
// low-frequency FBM sampled over the light's own direction (via `angular_noise_point` above) plus
// a slowly-translating time offset -- `lobes` sets roughly how many tongues/peaks show up around
// the circle, `reach_variance` how tall the peaks are relative to the valleys, `silhouette_speed`
// how fast the whole silhouette morphs. A second FBM textures the inside of each tongue: it's a
// static weave of angle and radius (no time term), so it reads as fixed internal grain rather than
// material flowing outward -- a uniform outward flow reads as an unnatural steady divergence out
// of the light's center rather than something a real light source does. Instead the corona's
// *brightness* pulses over time (`flicker_speed`/`flicker_intensity`), like a flame or plasma
// light guttering. `flicker_independence` blends the flicker's sample point between a single fixed
// coordinate (`0.0` -- the whole corona brightens/dims in lockstep) and the same
// `angular_noise_point` circle the silhouette/streak use (`1.0` -- each tongue gets its own
// noise-driven phase, so different parts of the corona gutter independently). The falloff past
// `reach` is a soft exponential fray (`tip_softness_px`), not a hard cutoff -- real flame tips
// dissipate raggedly. `params_a` = (lobes, reach_variance, silhouette_speed, base_reach_px),
// `params_b` = (streak_freq, streak_scale_px, streakiness, core_frac), `params_c` =
// (tip_softness_px, intensity, flicker_speed, flicker_intensity) -- see `project::FlameCoronaSpec`'s
// own field docs for what each means. `intensity <= 0.0` (in `params_c.y`, the zeroed-instance
// default) is the off switch.
fn flame_corona_strength(offset: vec2<f32>, core_radius: vec2<f32>, params_a: vec4<f32>, params_b: vec4<f32>, params_c: vec4<f32>, flicker_independence: f32, time_seconds: f32) -> f32 {
    let intensity = params_c.y;
    if (intensity <= 0.0) {
        return 0.0;
    }
    let lobes = params_a.x;
    let reach_variance = params_a.y;
    let silhouette_speed = params_a.z;
    let base_reach = params_a.w;
    let streak_freq = params_b.x;
    let streak_scale = params_b.y;
    let streakiness = params_b.z;
    let core_frac = params_b.w;
    let tip_softness = params_c.x;
    let flicker_speed = params_c.z;
    let flicker_intensity = params_c.w;

    let r = length(offset);
    let dir = offset / max(r, 0.0001);

    let silhouette_p = angular_noise_point(dir, lobes);
    let t = time_seconds * silhouette_speed;
    let silhouette_n = fbm(silhouette_p + vec2<f32>(t * 0.6, t));
    let reach = max(base_reach * (1.0 + reach_variance * silhouette_n), 1.0);

    // Woven, time-static streak texture: an angular FBM feeds into a radial one as its second
    // coordinate, coupling the two axes while staying continuous in `dir` (no `atan2` involved).
    let ang_n = fbm(angular_noise_point(dir, streak_freq));
    let streak_n = clamp(fbm(vec2<f32>(r / max(streak_scale, 1.0), ang_n * 3.0)) * 0.5 + 0.5, 0.0, 1.0);
    let streak = mix(1.0 - streakiness, 1.0, streak_n);

    let body = 1.0 - smoothstep(reach * core_frac, reach, r);
    let tip_fade = exp(-max(r - reach, 0.0) / max(tip_softness, 0.5));
    let shape = max(body, tip_fade);

    let inner_cut = smoothstep(0.0, min(core_radius.x, core_radius.y) * 0.4, r);

    var flicker = 1.0;
    if (flicker_intensity > 0.0) {
        let flicker_global = vec2<f32>(71.0, time_seconds * flicker_speed + 13.0);
        let flicker_local = angular_noise_point(dir, lobes) + vec2<f32>(time_seconds * flicker_speed * 0.8, time_seconds * flicker_speed * 1.3 + 5.0);
        let flicker_point = mix(flicker_global, flicker_local, clamp(flicker_independence, 0.0, 1.0));
        let ff = pow(clamp(noise2(flicker_point) * 0.5 + 0.5, 0.0, 1.0), 1.4);
        flicker = max(1.0 - flicker_intensity + flicker_intensity * ff, 0.0);
    }

    return shape * streak * flicker * inner_cut * intensity;
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

// `project::TurbulenceSpec`: displaces the sample point through a 2D value-noise field
// before any of the corona/flame-corona/ring math runs, so the whole light stack's shape reads as
// grainy/turbulent (a real photograph of a bright light) rather than perfectly smooth analytic
// falloffs. `strength_px <= 0.0` (the zeroed-instance default) is the off switch -- returns
// `offset` unchanged. Two independent noise samples (`nx`/`ny`, offset from each other in both
// space and time via arbitrary constants) drive the x/y displacement so the warp isn't a single
// scalar pushing every point the same direction.
fn domain_warp(offset: vec2<f32>, strength_px: f32, scale_px: f32, speed: f32, time_seconds: f32) -> vec2<f32> {
    if (strength_px <= 0.0) {
        return offset;
    }
    let p = offset / max(scale_px, 1.0);
    let t = time_seconds * speed;
    let nx = noise2(p + vec2<f32>(t, -t * 0.7));
    let ny = noise2(p + vec2<f32>(t * 0.6 + 17.0, t * 0.9 + 5.0));
    return offset + vec2<f32>(nx, ny) * strength_px;
}

// The combined (colorless) light strength at `offset` -- shared by every channel sample
// `fs_glow` takes below when chromatic aberration is enabled.
fn total_strength(in: VertexOutput, offset: vec2<f32>, time_seconds: f32) -> f32 {
    let warped = domain_warp(offset, in.turbulence.x, in.turbulence.y, in.turbulence.z, time_seconds);
    var s = core_strength(warped, in.core_radius, in.layer_amp, in.layer_sigma);
    s += flame_corona_strength(warped, in.core_radius, in.flame_a, in.flame_b, in.flame_c, in.flicker_independence, time_seconds);
    s += ring_strength(warped, in.ring_chromatic.x, in.ring_chromatic.y, in.ring_chromatic.z);
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
    let raw_light = color * strength_rgb * clamp(in.alpha, 0.0, 1.0);
    // `core_strength`'s exponential legitimately reaches values many times over 1.0 well within
    // `core_radius` (its interior isn't clamped, per that function's own doc comment -- brightness
    // rises continuously all the way to the center). This target is `Rgba8Unorm` with plain
    // additive blending and no HDR intermediate, so writing that raw value straight out gets
    // hard-clamped by the GPU on every channel that exceeds 1.0 -- across most of the core's
    // interior at once, which reproduces the exact flat, hard-edged disc the signed-distance
    // formula was written to avoid, just via the output format's clamp instead of the formula's.
    // It also hides decay: shrinking `alpha` during a flash's `decay_seconds` has no visible effect
    // until `raw_light` finally drops back under 1.0, so the core looks static and then vanishes
    // instead of fading. `explorations/barrier-fx-lab/barrier-fx-lab.html`'s final compositing step
    // (`outColor = 1.0 - exp(-outColor * uExposure)`, exposure defaulting to 1.0) hides the same
    // saturation behind a scene-wide tonemap; there's no equivalent full-scene HDR pass here (each
    // pass writes straight to the shared unorm target), so the same curve is applied locally to
    // each additive draw instead -- softly compressing toward 1.0 rather than hard-clamping, so the
    // core reads as a bright point rather than a flat disc and keeps visibly responding to `alpha`
    // (and thus decay) across its whole range instead of only right at the very end.
    let light = vec3<f32>(1.0, 1.0, 1.0) - exp(-raw_light);
    return vec4<f32>(light, 1.0);
}
