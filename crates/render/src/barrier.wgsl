// Renders the barrier as a full-canvas-width horizontal bar in pixel space (top-left origin,
// y-down — same convention `notes/shader.wgsl` uses for its `@builtin(position)` distance-field
// math). No vertex buffer: six hardcoded unit-quad corners, positioned/sized entirely from
// `uniforms.geometry`/`bar_color` (see `barrier.rs`'s `Uniforms` doc comment for field layout).
//
// Two fragment entry points share this one vertex shader (Phase M): `fs_core` (opaque bar,
// alpha-blended) and `fs_glow` (additive corona) — see `barrier.rs`'s module doc comment for why
// an opaque core and an additive halo need two separate pipelines/passes.

struct Uniforms {
    // x = canvas width, y = canvas height, z = barrier center y (pixels), w = thickness (pixels).
    geometry: vec4<f32>,
    // xyz = bar color (linear), w unused.
    bar_color: vec4<f32>,
    // x = glow enabled (0/1), y = pulse curve (0..1, decaying), z = wavy enabled (0/1),
    // w = wavy mode (0=TopWave, 1=Edge, 2=FullWave; only meaningful when z is set).
    flags: vec4<f32>,
    // x = wave amplitude (px), y = wavelength (px), z = speed, w = transport time (seconds).
    wave: vec4<f32>,
    // xyz = halo color (linear, independent of the bar's own color), w unused.
    glow_style: vec4<f32>,
    // x = resting brightness (bar's Glow::brightness, or 1.0 if no glow), y = peak brightness at
    // pulse = 1.0 (Pulse::brightness, or equal to x if no pulse), zw unused.
    glow_brightness_pulse: vec4<f32>,
    // Additive corona layers: x = layer[0].amplitude, y = layer[0].sigma_px, z = layer[1].amplitude,
    // w = layer[1].sigma_px.
    glow_layers_ab: vec4<f32>,
    // x = layer[2].amplitude, y = layer[2].sigma_px, z = precomputed glow margin (px), w unused.
    glow_layers_c: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

// Desaturates `base` toward pure white as `brightness` climbs past 1.0, rather than just scaling
// its channels up (which doesn't converge to white unless they already share the same magnitude)
// — this is what makes a bright glow read as a genuinely white-hot light source instead of a more
// saturated tint of its original color. `brightness <= 1.0` is a plain dimmer. `brightness == 1.0`
// is an exact no-op (`base * 1.0 == base` on one side, `mix(base, white, 0.0) == base` on the
// other — both branches agree at the boundary). Duplicated verbatim in `notes/shader.wgsl` and
// (as a CPU-side equivalent) `effects.rs`, since each is a separate shader module/crate boundary.
// Since Phase M this is only used for the opaque core's own fill — the corona is additive and
// whitens for free via saturation, no explicit mix-toward-white needed there.
fn hot_color(base: vec3<f32>, brightness: f32) -> vec3<f32> {
    let b = max(brightness, 0.0);
    if (b <= 1.0) {
        return base * b;
    }
    let whiteness = 1.0 - 1.0 / b;
    return mix(base, vec3<f32>(1.0), whiteness);
}

// The effective brightness driving both the core's hot-color mix and the corona's additive
// strength at the current pulse phase — shared between `vs_main` (which doesn't need it directly
// anymore, see below) and both fragment entry points, so everything agrees exactly.
fn effective_brightness() -> f32 {
    let pulse = clamp(uniforms.flags.y, 0.0, 1.0);
    return mix(uniforms.glow_brightness_pulse.x, uniforms.glow_brightness_pulse.y, pulse);
}

// Calm, stochastic-looking (not one literal sine) top-edge displacement at pixel-x `x` — three
// incommensurate-frequency sine terms weighted 0.6/0.3/0.1 (sum to 1.0), so |offset| <=
// uniforms.wave.x always holds exactly, which is what the vertex shader's inflation margin below
// relies on. Returns 0 (flat edge, today's only look) when wavy is disabled.
fn wavy_offset(x: f32) -> f32 {
    if (uniforms.flags.z < 0.5) {
        return 0.0;
    }
    let amp = uniforms.wave.x;
    let k = 6.283185307 / max(uniforms.wave.y, 1.0);
    let speed = uniforms.wave.z;
    let t = uniforms.wave.w;
    let p1 = x * k * 1.00       + t * speed * 1.00;
    let p2 = x * k * 2.17 + 1.7 + t * speed * 1.37;
    let p3 = x * k * 3.91 + 4.2 + t * speed * 0.61;
    return amp * (0.6 * sin(p1) + 0.3 * sin(p2) + 0.1 * sin(p3));
}

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
    // zero margin when disabled makes this an exact no-op, not just visually close. The margin is
    // precomputed on the CPU (`barrier.rs`'s `glow_layers_c.z`, `max(layer sigmas) * 5`) rather
    // than recomputed per-vertex here. Also inflate symmetrically top/bottom by the wave amplitude
    // when wavy is enabled — every `WavyMode`'s per-edge offset is bounded to
    // `[-amplitude_px, amplitude_px]` (see the fragment shaders below), so a single symmetric
    // margin covers `TopWave`/`Edge`/`FullWave` alike — zero margin when disabled, same exact-no-op
    // guarantee.
    let glow_margin = select(0.0, uniforms.glow_layers_c.z, uniforms.flags.x > 0.5);
    let wavy_margin = select(0.0, uniforms.wave.x, uniforms.flags.z > 0.5);
    let half_extent = thickness * 0.5 + glow_margin + wavy_margin;

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

// Shared by both fragment entry points: the wavy top/bottom edges and the resulting `core_alpha`
// shape (1.0 inside the bar, smoothly falling to 0.0 at its edges), plus how far outside those
// edges the current fragment sits (`edge_dist`, 0 inside the bar).
struct EdgeShape {
    core_alpha: f32,
    edge_dist: f32,
}

fn edge_shape(frag_y: f32, frag_x: f32) -> EdgeShape {
    let barrier_y = uniforms.geometry.z;
    let half_thickness = uniforms.geometry.w * 0.5;

    // `WavyMode::TopWave` (default): only the top edge waves, bottom stays flat (a rippling
    // surface over a flat floor, thickness varies across the bar's width, and can pinch thin
    // where the wave dips inward) — the original "calm ocean cross-section" look.
    // `Edge`: the identical (signed) offset is also applied to the bottom edge, so the whole bar
    // rides the wave rigidly (constant thickness, reads as a thin translating line rather than a
    // bar with volume).
    // `FullWave`: both edges bulge *outward* together, `swell = 0.5 * (amplitude_px + wave)`,
    // which is always in `[0, amplitude_px]` since `wave` is itself bounded to
    // `[-amplitude_px, amplitude_px]` — so thickness is always `base + 2*swell >= base`, never
    // pinching to (near) zero the way an independently-signed pairing could, while still varying
    // continuously with the same calm underlying pattern as the other two modes (both edges bulge
    // most at the same x where `wave` itself peaks).
    // All three offsets are exactly `0.0` when wavy is disabled (flags.z < 0.5), and `TopWave`'s
    // `bottom_offset` is always `0.0`, so `TopWave` is byte-for-byte identical to how this shader
    // worked before `Edge`/`FullWave` existed.
    let wave = wavy_offset(frag_x);
    var top_offset = 0.0;
    var bottom_offset = 0.0;
    if (uniforms.flags.z > 0.5) {
        if (uniforms.flags.w > 1.5) {
            let amp = uniforms.wave.x;
            let swell = 0.5 * (amp + wave);
            top_offset = -swell;
            bottom_offset = swell;
        } else if (uniforms.flags.w > 0.5) {
            top_offset = wave;
            bottom_offset = wave;
        } else {
            top_offset = wave;
        }
    }
    let top_edge = barrier_y - half_thickness + top_offset;
    let bottom_edge = barrier_y + half_thickness + bottom_offset;
    let alpha_top = smoothstep(top_edge - 1.0, top_edge + 1.0, frag_y);
    let alpha_bottom = 1.0 - smoothstep(bottom_edge - 1.0, bottom_edge + 1.0, frag_y);
    // When wave == 0 this is algebraically identical to the old single symmetric
    // `1 - smoothstep(half_thickness - 1, half_thickness + 1, abs(y - barrier_y))`: substituting
    // u = y - barrier_y, the old form is `1 - smoothstep(ht-1, ht+1, |u|)`, which for u >= 0 equals
    // `1 - smoothstep(ht-1, ht+1, u)` = `smoothstep(-(ht+1), -(ht-1), -u)` = (shifting by
    // 2*barrier_y) `smoothstep(top_edge-1, top_edge+1, y)` when the two smoothstep windows around
    // top_edge/bottom_edge don't overlap (true whenever thickness > 2px) — and for u < 0 by
    // symmetry equals the bottom half. So `alpha_top * alpha_bottom` reproduces the old formula
    // exactly in the non-overlapping-window regime, with no behavior change when wavy is off.
    let core_alpha = clamp(alpha_top, 0.0, 1.0) * clamp(alpha_bottom, 0.0, 1.0);

    // Edge distance (not center distance) so the halo hugs the wavy edge rather than a flat line.
    // When wave == 0, dist_above/dist_below reduce to `half_thickness - vertical_dist`/
    // `vertical_dist - half_thickness`-shaped terms whose max (clamped at 0) is exactly
    // `max(vertical_dist - half_thickness, 0)` — the old center-distance formula's argument — so by
    // shift-invariance this is an exact no-op when wavy is disabled.
    let dist_above = top_edge - frag_y;
    let dist_below = frag_y - bottom_edge;
    let edge_dist = max(max(dist_above, dist_below), 0.0);

    var out: EdgeShape;
    out.core_alpha = core_alpha;
    out.edge_dist = edge_dist;
    return out;
}

// Opaque core: the flat bar itself, going white-hot as brightness climbs — a note's pulse briefly
// pushes `effective_brightness` from the resting value up toward `Pulse::brightness`, decaying
// back as `pulse` settles to 0.
@fragment
fn fs_core(in: VertexOutput) -> @location(0) vec4<f32> {
    let shape = edge_shape(in.position.y, in.position.x);
    let brightness = effective_brightness();
    let color = hot_color(uniforms.bar_color.rgb, brightness);
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), shape.core_alpha);
}

// Additive corona (Phase M): sums three exponential falloff terms
// (`amplitude * exp(-edge_dist / sigma_px)`) into a single light value, added onto whatever is
// already in the framebuffer (`ONE`/`ONE` blend) rather than alpha-blended — this is what lets the
// halo read as light radiating from a white-hot core instead of a single flat (possibly whitened)
// color at one spatial scale. `brightness` scales the total amount of light added; whitening
// happens for free via additive saturation, no explicit mix-toward-white needed here.
@fragment
fn fs_glow(in: VertexOutput) -> @location(0) vec4<f32> {
    let shape = edge_shape(in.position.y, in.position.x);
    let brightness = effective_brightness();

    let d = shape.edge_dist;
    var strength = 0.0;
    strength += uniforms.glow_layers_ab.x * exp(-d / max(uniforms.glow_layers_ab.y, 0.01));
    strength += uniforms.glow_layers_ab.z * exp(-d / max(uniforms.glow_layers_ab.w, 0.01));
    strength += uniforms.glow_layers_c.x * exp(-d / max(uniforms.glow_layers_c.y, 0.01));

    // Don't add light where the opaque core will draw over it anyway (drawn after this pass) —
    // avoids wasted additive buildup directly under the core that the core pipeline will occlude.
    let light = uniforms.glow_style.rgb * strength * brightness * (1.0 - shape.core_alpha);
    return vec4<f32>(light, 1.0);
}
