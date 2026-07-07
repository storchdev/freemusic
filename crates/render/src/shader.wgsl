struct Uniforms {
    // 3x3 homography (letterbox scale + aspect-corrected rotation + tilt/keystone) mapping the
    // unit quad to clip-space (x, y, w); see `video_quad::build_transform` for how it's built.
    transform: mat3x3<f32>,
    crop_uv_min: vec2<f32>,
    crop_uv_max: vec2<f32>,
    brightness: f32,
    // Non-zero when the render target is plain `Unorm`, so the shader must sRGB-encode the
    // sampled linear video color before storing. See `docs/implementation-notes.md`.
    manual_srgb_encode: f32,
};

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lower = c * 12.92;
    let higher = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(higher, lower, c <= vec3<f32>(0.0031308));
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var video_texture: texture_2d<f32>;
@group(0) @binding(2) var video_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0),
    );

    // (x, y, w) = transform * (local.x, local.y, 1); the GPU's own perspective divide by w
    // turns the `tilt` terms baked into `transform` into a real keystone effect, and also
    // perspective-corrects the uv interpolation below for free.
    let transformed = uniforms.transform * vec3<f32>(positions[vertex_index], 1.0);

    var out: VertexOutput;
    out.clip_position = vec4<f32>(transformed.x, transformed.y, 0.0, transformed.z);
    out.uv = uniforms.crop_uv_min + uvs[vertex_index] * (uniforms.crop_uv_max - uniforms.crop_uv_min);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(video_texture, video_sampler, in.uv);
    var rgb = color.rgb * uniforms.brightness;
    if (uniforms.manual_srgb_encode > 0.5) {
        rgb = linear_to_srgb(rgb);
    }
    return vec4<f32>(rgb, color.a);
}
