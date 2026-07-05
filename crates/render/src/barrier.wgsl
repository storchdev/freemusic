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
    // x = glow enabled (0/1), y = pulse intensity (0..1, decaying), z = wavy enabled (0/1),
    // w = wavy mode (0=TopWave, 1=Edge, 2=FullWave; only meaningful when z is set).
    flags: vec4<f32>,
    // x = wave amplitude (px), y = wavelength (px), z = speed, w = transport time (seconds).
    wave: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

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
    // zero margin when disabled makes this an exact no-op, not just visually close. Also inflate
    // symmetrically top/bottom by the wave amplitude when wavy is enabled — every `WavyMode`'s
    // per-edge offset is bounded to `[-amplitude_px, amplitude_px]` (see `fs_main`), so a single
    // symmetric margin covers `TopWave`/`Edge`/`FullWave` alike — zero margin when disabled, same
    // exact-no-op guarantee.
    let glow_margin = select(0.0, uniforms.color_glow_radius.w, uniforms.flags.x > 0.5);
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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
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
    let wave = wavy_offset(in.position.x);
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
    let alpha_top = smoothstep(top_edge - 1.0, top_edge + 1.0, in.position.y);
    let alpha_bottom = 1.0 - smoothstep(bottom_edge - 1.0, bottom_edge + 1.0, in.position.y);
    // When wave == 0 this is algebraically identical to the old single symmetric
    // `1 - smoothstep(half_thickness - 1, half_thickness + 1, abs(y - barrier_y))`: substituting
    // u = y - barrier_y, the old form is `1 - smoothstep(ht-1, ht+1, |u|)`, which for u >= 0 equals
    // `1 - smoothstep(ht-1, ht+1, u)` = `smoothstep(-(ht+1), -(ht-1), -u)` = (shifting by
    // 2*barrier_y) `smoothstep(top_edge-1, top_edge+1, y)` when the two smoothstep windows around
    // top_edge/bottom_edge don't overlap (true whenever thickness > 2px) — and for u < 0 by
    // symmetry equals the bottom half. So `alpha_top * alpha_bottom` reproduces the old formula
    // exactly in the non-overlapping-window regime, with no behavior change when wavy is off.
    let core_alpha = clamp(alpha_top, 0.0, 1.0) * clamp(alpha_bottom, 0.0, 1.0);

    let pulse = clamp(uniforms.flags.y, 0.0, 1.0);
    // Pulse briefly brightens the bar itself, decaying back to the base color as it settles.
    let color = mix(uniforms.color_glow_radius.rgb, vec3<f32>(1.0), pulse * 0.5);

    var alpha = core_alpha;
    if uniforms.flags.x > 0.5 {
        let glow_radius = uniforms.color_glow_radius.w;
        if glow_radius > 0.0 {
            // Edge distance (not center distance) so the halo hugs the wavy edge rather than a
            // flat line. When wave == 0, dist_above/dist_below reduce to
            // `half_thickness - vertical_dist`/`vertical_dist - half_thickness`-shaped terms whose
            // max (clamped at 0) is exactly `max(vertical_dist - half_thickness, 0)` — the old
            // center-distance formula's argument — so by smoothstep's shift-invariance this is an
            // exact no-op when wavy is disabled, and glow-alone (no wavy) is unaffected.
            let dist_above = top_edge - in.position.y;
            let dist_below = in.position.y - bottom_edge;
            let edge_dist = max(max(dist_above, dist_below), 0.0);
            var glow_alpha = 1.0 - smoothstep(0.0, glow_radius, edge_dist);
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
