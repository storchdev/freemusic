// Vendored from Neothesia's waterfall pipeline shader
// (`neothesia-core/src/render/waterfall/pipeline/shader.wgsl`, pinned commit — see CLAUDE.md).
// Real changes on top of the pixel-parity port (Phase B):
//  - the hit line ("keyboard_y") is no longer hardcoded to 80% of `view_uniform.size.y` — it
//    reads `view_uniform.barrier_fraction` instead, so the barrier can sit anywhere without the
//    viewport-remapping hack `midi_overlay.rs` used to need.
//  - a `StyleUniform` (group 2) drives fill (solid/vertical-gradient), an optional diagonal sheen
//    stripe, and an optional outer glow — Phase C of the `.fmstyle.ron` milestone. All three are
//    no-ops when their respective flag is 0, which is exactly what `Style::from_legacy` produces,
//    so the default (no imported style) look is pixel-identical to Phase B.
//  - the halo itself is an additive multi-layer corona (Phase M), rendered by a second pipeline
//    (`fs_glow`) sharing this file's `vs_main` — see `pipeline.rs`'s module doc comment and
//    `barrier.wgsl`'s equivalent split for the full rationale.
// `velocity`/`track_index` are carried as instance attributes for a future property-binding phase
// but are not yet read here.

struct ViewUniform {
    transform: mat4x4<f32>,
    size: vec2<f32>,
    scale: f32,
    barrier_fraction: f32,
}

struct TimeUniform {
    time: f32,
    speed: f32,
}

// All-vec4 layout deliberately, to sidestep the kind of std140 padding mismatch documented in
// CLAUDE.md for `mat3x3<f32>` uniforms — every field here is already vec4-aligned so there's no
// implicit padding for the Rust side to get wrong.
struct StyleUniform {
    // x unused (was a style-wide solid-vs-gradient flag; `fill_color` now always blends
    // `color_top`/`color_bottom` per note instead, see its own comment for why),
    // y = sheen_enabled, z = glow_enabled, w unused.
    fill_and_flags: vec4<f32>,
    // x = sheen intensity, y = sheen width (fraction of the note's diagonal span), z = sheen angle
    // (radians), w unused.
    sheen_params: vec4<f32>,
    // xyz = halo color (linear), w unused.
    glow_color: vec4<f32>,
    // x = glow brightness (scales the corona's own additive light in `fs_glow`), yzw unused.
    glow_params: vec4<f32>,
    // Additive corona layers: x = layer[0].amplitude, y = layer[0].sigma_px, z = layer[1].amplitude,
    // w = layer[1].sigma_px.
    glow_layers_ab: vec4<f32>,
    // x = layer[2].amplitude, y = layer[2].sigma_px, z = precomputed glow margin (px), w unused.
    glow_layers_c: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> view_uniform: ViewUniform;

@group(1) @binding(0)
var<uniform> time_uniform: TimeUniform;

@group(2) @binding(0)
var<uniform> style_uniform: StyleUniform;

struct Vertex {
    @location(0) position: vec2<f32>,
}

struct NoteInstance {
    @location(1) n_position: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) color_top: vec3<f32>,
    @location(4) color_bottom: vec3<f32>,
    @location(5) radius: f32,
    @location(6) velocity: f32,
    @location(7) track_index: f32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,

    @location(0) note_pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color_top: vec3<f32>,
    @location(3) color_bottom: vec3<f32>,
    @location(4) radius: f32,
}

@vertex
fn vs_main(vertex: Vertex, note: NoteInstance) -> VertexOutput {
    let speed = time_uniform.speed;

    // The note's true (unpadded) on-screen box — used for the rounded-rect distance field and the
    // gradient/sheen math in the fragment shader, so those are unaffected by any glow inflation
    // below.
    let true_size = vec2<f32>(note.size.x * view_uniform.scale, note.size.y * abs(speed));

    let keyboard_y = view_uniform.size.y * view_uniform.barrier_fraction;

    var true_pos = vec2<f32>(note.n_position.x * view_uniform.scale, keyboard_y);

    if speed > 0.0 {
        // If notes are falling from top to down, we need to adjust the position,
        // as their start is on bottom of the quad rather than top
        true_pos.y -= true_size.y;
    }

    // Offset position by playback time
    true_pos.y -= (note.n_position.y - time_uniform.time) * speed;

    // Glow needs the rasterized quad to extend beyond the note's true box so there are pixels to
    // paint the corona onto; when disabled (the default/legacy look) the margin is zero and this
    // is exactly the old unpadded quad. The margin is precomputed on the CPU (`pipeline.rs`'s
    // `glow_layers_c.z`, `max(layer sigmas) * 5`) rather than recomputed per-vertex here.
    let glow_margin = select(
        0.0,
        style_uniform.glow_layers_c.z,
        style_uniform.fill_and_flags.z > 0.5,
    );
    let draw_pos = true_pos - vec2<f32>(glow_margin, glow_margin);
    let draw_size = true_size + vec2<f32>(glow_margin, glow_margin) * 2.0;

    let transform = mat4x4<f32>(
        vec4<f32>(draw_size.x, 0.0,         0.0, 0.0),
        vec4<f32>(0.0,         draw_size.y, 0.0, 0.0),
        vec4<f32>(0.0,         0.0,         1.0, 0.0),
        vec4<f32>(draw_pos.x,  draw_pos.y,  0.0, 1.0)
    );

    var out: VertexOutput;
    out.position = view_uniform.transform * transform * vec4<f32>(vertex.position, 0.0, 1.0);
    out.note_pos = true_pos;

    out.size = true_size;
    out.color_top = note.color_top;
    out.color_bottom = note.color_bottom;
    out.radius = note.radius * view_uniform.scale;

    return out;
}

fn dist(
    frag_coord: vec2<f32>,
    position: vec2<f32>,
    size: vec2<f32>,
    radius: f32,
) -> f32 {
    let inner_size: vec2<f32> = size - vec2<f32>(radius, radius) * 2.0;
    let top_left: vec2<f32> = position + vec2<f32>(radius, radius);
    let bottom_right: vec2<f32> = top_left + inner_size;

    let top_left_distance: vec2<f32> = top_left - frag_coord;
    let bottom_right_distance: vec2<f32> = frag_coord - bottom_right;

    let dist: vec2<f32> = vec2<f32>(
        max(max(top_left_distance.x, bottom_right_distance.x), 0.0),
        max(max(top_left_distance.y, bottom_right_distance.y), 0.0),
    );

    return sqrt(dist.x * dist.x + dist.y * dist.y);
}

// Shared fill/sheen fragment color computation (independent of glow), used by `fs_core` — kept
// separate from `dist`'s edge-distance math so `fs_glow` doesn't need to duplicate it, and vice
// versa.
fn fill_color(in: VertexOutput) -> vec3<f32> {
    // Always blend by the fragment's position within the note's true (unpadded) box. For a solid
    // fill (default), `color_top == color_bottom` per instance, so this mix is a no-op and matches
    // Phase B's flat color exactly — this must stay a per-instance equality check rather than a
    // style-wide flag, since `BlackKeyFill::Custom` can give sharp-key notes a gradient (or vice
    // versa) independently of whether the natural-key `fill` itself is solid or a gradient.
    let shape_uv_y = clamp((in.position.y - in.note_pos.y) / max(in.size.y, 0.0001), 0.0, 1.0);
    var color = mix(in.color_top, in.color_bottom, shape_uv_y);

    // Diagonal specular stripe, swept across the note's fill at a fixed angle.
    if style_uniform.fill_and_flags.y > 0.5 {
        let angle = style_uniform.sheen_params.z;
        let local = in.position.xy - in.note_pos;
        let span = in.size.x * abs(cos(angle)) + in.size.y * abs(sin(angle));
        let d_axis = local.x * cos(angle) + local.y * sin(angle);
        let center = span * 0.5;
        let half_width = max(style_uniform.sheen_params.y * span * 0.5, 0.001);
        let sheen_amount = clamp(1.0 - abs(d_axis - center) / half_width, 0.0, 1.0);
        color = color + vec3<f32>(sheen_amount * style_uniform.sheen_params.x);
    }

    return color;
}

// Opaque core: the note's own fill (solid/gradient + optional sheen) — a note with `glow:
// Some(..)` blends a thin rim near its own edge toward the corona's own color (never toward
// white — an earlier "white-hot rim" that whitened the fill itself was removed because it read as
// an unwanted artifact) so the fill's true color hands off into the corona's color continuously
// instead of meeting it at a hard seam. Interior pixels (more than a few px from the edge) keep
// their true fill color untouched, same as when `glow` is unset.
//
// `dist` saturates at 0 for the whole interior more than `in.radius` px from the note's true edge
// (see its doc comment), so `in.radius - d` is a distance-from-edge measure that's exact within
// that `radius`-px band and clamped (not extrapolated) beyond it. `Glow::edge_blend_px` (falling
// back to `glow_layers_ab.y`, the corona's own tightest layer sigma, when `0.0`) sets how far this
// blend reaches inward — independent of the corona's own reach, so the rim's smoothness can be
// tuned without changing the corona itself. The rim's *target* color is the corona's own strength
// at `d_past_edge == 0` (the sum of all three layers' amplitudes, times brightness, clamped to a
// displayable range) rather than just `color` on its own — matching `fs_glow`'s actual computed
// brightness at that point instead of a dimmer flat color, which previously read as a visible dark
// gap between the rim and the corona whenever `brightness`/`amplitude` pushed the corona brighter
// than plain `color`.
@fragment
fn fs_core(in: VertexOutput) -> @location(0) vec4<f32> {
    let d: f32 = dist(in.position.xy, in.note_pos, in.size, in.radius);
    let base_alpha: f32 = 1.0 - smoothstep(max(in.radius - 0.5, 0.0), in.radius + 0.5, d);

    var out_color = fill_color(in);
    if style_uniform.fill_and_flags.z > 0.5 {
        let inward_dist = max(in.radius - d, 0.0);
        let edge_blend_px = style_uniform.glow_params.y;
        let rim_sigma_source = select(style_uniform.glow_layers_ab.y, edge_blend_px, edge_blend_px > 0.0);
        let rim_sigma = max(rim_sigma_source, 0.5);
        let rim_weight = exp(-inward_dist / rim_sigma);
        let total_amplitude = style_uniform.glow_layers_ab.x + style_uniform.glow_layers_ab.z + style_uniform.glow_layers_c.x;
        let rim_target = clamp(
            style_uniform.glow_color.rgb * total_amplitude * style_uniform.glow_params.x,
            vec3<f32>(0.0),
            vec3<f32>(1.0)
        );
        out_color = mix(out_color, rim_target, rim_weight);
    }

    return vec4<f32>(clamp(out_color, vec3<f32>(0.0), vec3<f32>(1.0)), base_alpha);
}

// Additive corona (Phase M): sums three exponential falloff terms
// (`amplitude * exp(-d_past_edge / sigma_px)`) into a single light value, added onto whatever is
// already in the framebuffer (`ONE`/`ONE` blend) — see `barrier.wgsl`'s `fs_glow` for the full
// rationale, identical mechanism here using `d_past_edge` (distance outside the note's rounded-rect
// edge) in place of the barrier's `edge_dist`.
@fragment
fn fs_glow(in: VertexOutput) -> @location(0) vec4<f32> {
    let d: f32 = dist(in.position.xy, in.note_pos, in.size, in.radius);
    let base_alpha: f32 = 1.0 - smoothstep(max(in.radius - 0.5, 0.0), in.radius + 0.5, d);
    let d_past_edge = max(d - in.radius, 0.0);

    let brightness = style_uniform.glow_params.x;
    var strength = 0.0;
    strength += style_uniform.glow_layers_ab.x * exp(-d_past_edge / max(style_uniform.glow_layers_ab.y, 0.01));
    strength += style_uniform.glow_layers_ab.z * exp(-d_past_edge / max(style_uniform.glow_layers_ab.w, 0.01));
    strength += style_uniform.glow_layers_c.x * exp(-d_past_edge / max(style_uniform.glow_layers_c.y, 0.01));

    // Don't add light where the opaque core will draw over it anyway (drawn after this pass).
    let light = style_uniform.glow_color.rgb * strength * brightness * (1.0 - base_alpha);
    return vec4<f32>(light, 1.0);
}
