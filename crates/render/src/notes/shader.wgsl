// Vendored from Neothesia's waterfall pipeline shader
// (`neothesia-core/src/render/waterfall/pipeline/shader.wgsl`, pinned commit — see CLAUDE.md).
// Two real changes on top of the pixel-parity port (Phase B):
//  - the hit line ("keyboard_y") is no longer hardcoded to 80% of `view_uniform.size.y` — it
//    reads `view_uniform.barrier_fraction` instead, so the barrier can sit anywhere without the
//    viewport-remapping hack `midi_overlay.rs` used to need.
//  - a `StyleUniform` (group 2) drives fill (solid/vertical-gradient), an optional diagonal sheen
//    stripe, and an optional outer glow — Phase C of the `.fmstyle.ron` milestone. All three are
//    no-ops when their respective flag is 0, which is exactly what `Style::from_legacy` produces,
//    so the default (no imported style) look is pixel-identical to Phase B.
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
    // x = fill_kind (0 = solid, 1 = vertical gradient), y = sheen_enabled, z = glow_enabled, w unused.
    fill_and_flags: vec4<f32>,
    // x = sheen intensity, y = sheen width (fraction of the note's diagonal span), z = sheen angle
    // (radians), w unused.
    sheen_params: vec4<f32>,
    // xyz = glow color (linear), w = glow radius in pixels.
    glow_color_radius: vec4<f32>,
    // x = glow intensity, yzw unused.
    glow_intensity: vec4<f32>,
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
    // paint the halo onto; when disabled (the default/legacy look) the margin is zero and this is
    // exactly the old unpadded quad.
    let glow_margin = select(0.0, style_uniform.glow_color_radius.w, style_uniform.fill_and_flags.z > 0.5);
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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let d: f32 = dist(
        in.position.xy,
        in.note_pos,
        in.size,
        in.radius,
    );

    let base_alpha: f32 = 1.0 - smoothstep(
        max(in.radius - 0.5, 0.0),
        in.radius + 0.5,
        d,
    );

    // Solid fill (default): color_top == color_bottom, so this mix is a no-op and matches Phase
    // B's flat color exactly. Vertical gradient blends by the fragment's position within the
    // note's true (unpadded) box.
    var fill_color = in.color_top;
    if style_uniform.fill_and_flags.x > 0.5 {
        let shape_uv_y = clamp((in.position.y - in.note_pos.y) / max(in.size.y, 0.0001), 0.0, 1.0);
        fill_color = mix(in.color_top, in.color_bottom, shape_uv_y);
    }

    // Diagonal specular stripe, swept across the note's fill at a fixed angle.
    if style_uniform.fill_and_flags.y > 0.5 {
        let angle = style_uniform.sheen_params.z;
        let local = in.position.xy - in.note_pos;
        let span = in.size.x * abs(cos(angle)) + in.size.y * abs(sin(angle));
        let d_axis = local.x * cos(angle) + local.y * sin(angle);
        let center = span * 0.5;
        let half_width = max(style_uniform.sheen_params.y * span * 0.5, 0.001);
        let sheen_amount = clamp(1.0 - abs(d_axis - center) / half_width, 0.0, 1.0);
        fill_color = fill_color + vec3<f32>(sheen_amount * style_uniform.sheen_params.x);
    }

    var out_color = fill_color;
    var out_alpha = base_alpha;

    // Soft outer halo: visible only outside/at the note's edge (scaled down by `1 - base_alpha`
    // so it doesn't wash out the note's own interior), blended under the crisp fill.
    if style_uniform.fill_and_flags.z > 0.5 {
        let glow_radius_px = style_uniform.glow_color_radius.w;
        let glow_far = max(in.radius + glow_radius_px, in.radius + 0.001);
        var glow_alpha = style_uniform.glow_intensity.x * (1.0 - smoothstep(in.radius, glow_far, d));
        glow_alpha = glow_alpha * (1.0 - base_alpha);
        out_alpha = clamp(base_alpha + glow_alpha, 0.0, 1.0);
        out_color = mix(style_uniform.glow_color_radius.xyz, fill_color, base_alpha);
    }

    return vec4<f32>(clamp(out_color, vec3<f32>(0.0), vec3<f32>(1.0)), out_alpha);
}
