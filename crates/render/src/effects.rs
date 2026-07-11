//! Renders barrier-hit transitions: a fixed-pool CPU particle simulation plus decaying flashes,
//! spawned when note arrivals cross the transport position. The simulation is stateful and uses
//! separate additive and premultiplied-alpha pipelines; see `docs/implementation-notes.md`.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use project::{
    ColorBinding, EmissionMode, FlashColor, FlashMode, FlashSpec, GodRaySpec, ParticleColor,
    ParticleSpec, RingSpec, TransitionKind, TransitionLayer,
};

use super::notes::NoteInterval;

/// A `time_seconds` jump larger than this (in either direction) between two `update` calls is
/// treated as a scrub rather than ordinary playback advancing by a redraw's `dt` — the pool is
/// cleared instead of spawning every event the jump skipped over or trying to run particles
/// backward.
const MAX_ORDINARY_STEP_SECONDS: f32 = 0.35;

/// Number of evenly-spaced left-to-right color stops every `EffectInstance` carries — purely a
/// rendering-resolution knob for `project::FlashColor::HorizontalGradient` (an author-painted
/// gradient of arbitrary length gets resampled to this many stops) and for
/// `project::FlashColor::MatchNote` (which just fills every stop with the note's single color at
/// the barrier — see `FlashColorSource::MatchNote`'s doc comment for why this isn't a finer
/// per-pixel sample of the note).
const FLASH_GRADIENT_STOPS: usize = 5;

/// Cutoff distance past which a `GlowLayer`'s `exp(-d / sigma_px)` contribution is treated as
/// invisible — see `barrier.rs`'s identical constant for the full rationale.
const GLOW_CUTOFF_SIGMAS: f32 = 5.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    transform: [f32; 16],
    /// x = transport time (seconds), read by `effects.wgsl`'s Phase V god-ray pulse/flicker/
    /// rotation noise — yzw unused, packed into a vec4 (rather than a bare trailing f32) to match
    /// this codebase's uniform-buffer convention and avoid manual tail padding.
    time: [f32; 4],
}

fn orthographic_projection(width: f32, height: f32) -> [f32; 16] {
    #[rustfmt::skip]
    let out = [
        2.0 / width, 0.0, 0.0, 0.0,
        0.0, -2.0 / height, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        -1.0, 1.0, 0.0, 1.0,
    ];
    out
}

impl Default for ViewUniform {
    fn default() -> Self {
        Self {
            transform: orthographic_projection(1.0, 1.0),
            time: [0.0; 4],
        }
    }
}

/// `core_radius`/`quad_radius` split plus per-instance additive corona layers, baked in at spawn
/// time (`spawn_one_particle`/`spawn_flash`) rather than read from a shared uniform, since
/// particles/flashes already bake their final linear color into the instance this way.
/// `core_radius` is the configured half-extent (ellipse-aware); `quad_radius` is
/// `core_radius + margin` for glow instances (flashes, additive particles) or exactly
/// `core_radius` for non-additive "puff" particles (`fs_puff` never reads `layer_amp`/
/// `layer_sigma` at all, so leaving them zeroed for puffs is not a footgun).
/// `color_stops[0]` is the color at the instance's own left edge (`center.x - quad_radius.x`),
/// `color_stops[FLASH_GRADIENT_STOPS - 1]` at its right edge, evenly spaced in between — a flash's
/// horizontal gradient (see `project::FlashColor`) reads all of these; a particle (which only
/// ever has one color) simply has every stop set equal, which interpolates to that one color
/// everywhere and is pixel-identical to a plain `color: vec3<f32>` field would have been.
///
/// `godray_a`/`godray_b`/`godray_c`/`ring_chromatic` (Phase V) carry `project::GodRaySpec`/
/// `RingSpec`/`FlashSpec::chromatic_aberration`, packed 1:1 with `effects.wgsl`'s `Instance`
/// fields of the same names — see that shader's doc comments for the packing layout. Only
/// `spawn_flash` ever fills these with non-zero data; every particle instance leaves them zeroed
/// (`ZERO_GOD_RAYS`/`ZERO_RING_CHROMATIC` below), which `fs_glow` treats as "effect off" (`count <
/// 0.5`, `ring_intensity <= 0.0`, `chromatic_amount <= 0.0`), reproducing the pre-Phase-V corona
/// exactly.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EffectInstance {
    center: [f32; 2],
    core_radius: [f32; 2],
    quad_radius: [f32; 2],
    alpha: f32,
    color_stops: [[f32; 3]; FLASH_GRADIENT_STOPS],
    /// x/y/z = layer[0..3].amplitude, pre-multiplied by the spec's `brightness` at spawn time
    /// (a plain multiply, not a `hot_color` mix — additive saturation whitens for free).
    layer_amp: [f32; 3],
    layer_sigma: [f32; 3],
    godray_a: [f32; 4],
    godray_b: [f32; 4],
    godray_c: [f32; 4],
    ring_chromatic: [f32; 4],
}

/// Zero-filled god-ray/ring/chromatic-aberration params — every non-flash `EffectInstance` (all
/// particles) carries these unchanged, which `fs_glow` reads as "effect off" (see
/// `EffectInstance`'s own doc comment).
const ZERO_GOD_RAYS: ([f32; 4], [f32; 4], [f32; 4]) = ([0.0; 4], [0.0; 4], [0.0; 4]);
const ZERO_RING_CHROMATIC: [f32; 4] = [0.0; 4];

// `color_stops` below is hand-unrolled to `FLASH_GRADIENT_STOPS == 5` explicit locations (5..=9) —
// `wgpu::vertex_attr_array!` takes a literal list of `location => format` entries, not a
// const-generic count, so it can't loop over `FLASH_GRADIENT_STOPS` itself. This assertion is the
// tripwire: bump it (and the two hardcoded ranges just below/in `effects.wgsl`) if that constant
// ever changes.
const _: () = assert!(
    FLASH_GRADIENT_STOPS == 5,
    "update the hand-unrolled attribute list below"
);

impl EffectInstance {
    fn attributes() -> [wgpu::VertexAttribute; 15] {
        wgpu::vertex_attr_array![
            1 => Float32x2,  // center
            2 => Float32x2,  // core_radius
            3 => Float32x2,  // quad_radius
            4 => Float32,    // alpha
            5 => Float32x3,  // color_stops[0]
            6 => Float32x3,  // color_stops[1]
            7 => Float32x3,  // color_stops[2]
            8 => Float32x3,  // color_stops[3]
            9 => Float32x3,  // color_stops[4]
            10 => Float32x3, // layer_amp
            11 => Float32x3, // layer_sigma
            12 => Float32x4, // godray_a
            13 => Float32x4, // godray_b
            14 => Float32x4, // godray_c
            15 => Float32x4, // ring_chromatic
        ]
    }

    fn layout(attributes: &[wgpu::VertexAttribute]) -> wgpu::VertexBufferLayout<'_> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<EffectInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    position: [f32; 2],
}

/// sRGB u8 -> linear f32, the same small conversion this codebase keeps a private copy of in
/// every module that needs it (`notes::color_to_linear`, `barrier::srgb_to_linear`) rather than
/// sharing, since none of them are `pub`.
fn srgb_to_linear([r, g, b]: [u8; 3]) -> [f32; 3] {
    fn component(u: u8) -> f32 {
        let u = u as f32 / 255.0;
        if u < 0.04045 {
            u / 12.92
        } else {
            ((u + 0.055) / 1.055).powf(2.4)
        }
    }
    [component(r), component(g), component(b)]
}

/// CPU-side equivalent of `barrier.wgsl`/`notes/shader.wgsl`'s `hot_color`: desaturates `base`
/// toward pure white as `brightness` climbs past `1.0`, rather than just scaling its channels up
/// (which doesn't converge to white unless they already share the same magnitude) — same
/// rationale as those shaders' doc comments. `brightness <= 1.0` is a plain dimmer;
/// `brightness == 1.0` is an exact no-op. This is only used for non-additive "puff"
/// particles (which have no separate opaque core to whiten, unlike barrier/notes, so the fill
/// color itself is whitened) — flashes and additive particles instead pre-multiply `brightness`
/// into their additive corona `layer_amp`, letting additive saturation whiten for free.
fn hot_color([r, g, b]: [f32; 3], brightness: f32) -> [f32; 3] {
    let b_mul = brightness.max(0.0);
    if b_mul <= 1.0 {
        return [r * b_mul, g * b_mul, b * b_mul];
    }
    let whiteness = 1.0 - 1.0 / b_mul;
    [
        r + (1.0 - r) * whiteness,
        g + (1.0 - g) * whiteness,
        b + (1.0 - b) * whiteness,
    ]
}

/// The resolved (already-linear) endpoints of a `project::ParticleColor::YGradient`, plus the
/// canvas-pixel span (`top_px`/`bottom_px`, from `top_fraction`/`bottom_fraction` * canvas height)
/// they blend across — everything needed to keep re-evaluating a particle's color after spawn as
/// it moves, see `Particle::y_gradient`.
#[derive(Clone, Copy)]
struct YGradientRange {
    top: [f32; 3],
    bottom: [f32; 3],
    top_px: f32,
    bottom_px: f32,
}

impl YGradientRange {
    /// The blended color at `y_px`, clamping outside `[top_px, bottom_px]` rather than
    /// extrapolating — a degenerate (zero-height) range just returns `top`.
    fn color_at(&self, y_px: f32) -> [f32; 3] {
        let span = self.bottom_px - self.top_px;
        if span.abs() < 0.001 {
            return self.top;
        }
        let t = ((y_px - self.top_px) / span).clamp(0.0, 1.0);
        mix3(self.top, self.bottom, t)
    }
}

struct Particle {
    pos: [f32; 2],
    vel: [f32; 2],
    gravity_px: f32,
    life_seconds: f32,
    lifetime_seconds: f32,
    size_px: f32,
    /// Current color — fixed for `ParticleColor::Fixed`/`MatchNote` (baked once at spawn),
    /// recomputed every ordinary step from `y_gradient` (if set) using the particle's *current*
    /// canvas Y, since `ParticleColor::YGradient` is the one mode where color isn't fixed at
    /// spawn — a particle visibly shifts color as gravity/its initial velocity move it.
    color: [f32; 3],
    /// `Some(range)` when this particle's spec was `ParticleColor::YGradient` — `None` for every
    /// other mode, since only `YGradient` needs re-evaluating after spawn. `brightness`/`additive`
    /// are only meaningful alongside a `Some` `y_gradient`, so the per-frame recompute can reapply
    /// the exact same post-processing `spawn_one_particle` applied once at spawn time (`hot_color`
    /// for non-additive puffs, a no-op multiply already folded into `layer_amp` for additive ones
    /// — see that function).
    y_gradient: Option<YGradientRange>,
    brightness: f32,
    additive: bool,
    /// `0.0` for non-additive "puff" particles (no corona, `quad_radius == core_radius`).
    margin_px: f32,
    layer_amp: [f32; 3],
    layer_sigma: [f32; 3],
}

/// How a `Flash`'s `color_stops` are produced each frame in `rebuild_instances`. `Solid`/
/// `HorizontalGradient` bake to a fixed set of stops once at spawn (a flash doesn't move, so
/// there's nothing about its position to re-sample); `MatchNote` instead keeps the triggering
/// note's `NoteInterval` around and re-evaluates `color_at_barrier` every frame, since — unlike a
/// fixed color — "whichever part of the note is currently at the barrier" keeps changing for as
/// long as the flash stays lit. This mostly matters for `FlashMode::Sustained` (which holds a
/// flash lit for a note's entire duration): without this, a long-held note's glow would stay
/// pinned to its arrival color instead of sliding across the note's own gradient as more of it
/// feeds past the barrier.
enum FlashColorSource {
    Fixed([[f32; 3]; FLASH_GRADIENT_STOPS]),
    MatchNote(NoteInterval),
}

struct Flash {
    pos: [f32; 2],
    /// Absolute transport-time threshold at which decay begins — `time_seconds` at spawn for
    /// `FlashMode::Instant`, or the note's `end_seconds` (in the future, at spawn time) for
    /// `FlashMode::Sustained`. Alpha is a pure function of `time_seconds - decay_start_seconds`,
    /// so a sustained flash simply stays at full intensity for as long as this stays in the
    /// future — no separate per-frame "is the note still held" bookkeeping needed.
    decay_start_seconds: f32,
    decay_seconds: f32,
    radius_x_px: f32,
    radius_y_px: f32,
    color: FlashColorSource,
    margin_px: f32,
    layer_amp: [f32; 3],
    layer_sigma: [f32; 3],
    /// See `flash_flicker`'s doc comment. `flicker_intensity == 0.0` (the default) makes this a
    /// no-op regardless of `flicker_speed`.
    flicker_speed: f32,
    flicker_intensity: f32,
    /// Random per-spawn seed so simultaneous flashes don't flicker in lockstep — see
    /// `flash_flicker`'s `seed` parameter.
    flicker_seed: f32,
    /// Phase V god-ray/ring/chromatic-aberration params, resolved once at spawn time from
    /// `FlashSpec::god_rays`/`ring`/`chromatic_aberration` — see `EffectInstance`'s doc comment
    /// for the packing layout. `ZERO_GOD_RAYS`/`ZERO_RING_CHROMATIC` for a flash with none of
    /// these set, which `fs_glow` renders identically to a pre-Phase-V flash.
    godray_a: [f32; 4],
    godray_b: [f32; 4],
    godray_c: [f32; 4],
    ring_chromatic: [f32; 4],
}

/// `EffectInstance::godray_a`/`b`/`c` from a `project::GodRaySpec`, or `ZERO_GOD_RAYS` if the
/// flash has none — see that struct's own field docs for what each packed slot means.
fn resolve_god_ray_params(spec: Option<&GodRaySpec>) -> ([f32; 4], [f32; 4], [f32; 4]) {
    let Some(g) = spec else {
        return ZERO_GOD_RAYS;
    };
    (
        [
            // `count` gates the whole effect on/off in `fs_glow` (`count < 0.5` == off) — clamp
            // to at least 1 so a `Some(GodRaySpec { .. })` with a bare `count: 0` (nonsensical but
            // representable) doesn't silently disable itself.
            (g.count.max(1)) as f32,
            g.length_px,
            g.length_jitter,
            g.softness,
        ],
        [
            g.rotation_offset_deg,
            g.rotation_speed_deg_per_sec,
            g.pulse_speed,
            g.pulse_amount,
        ],
        [
            g.streakiness,
            g.flicker_speed,
            g.flicker_intensity,
            g.intensity,
        ],
    )
}

/// `EffectInstance::ring_chromatic` from a `project::RingSpec` (or `None`) plus
/// `FlashSpec::chromatic_aberration` — see `EffectInstance`'s own doc comment for the packing.
fn resolve_ring_chromatic_params(ring: Option<&RingSpec>, chromatic_aberration: f32) -> [f32; 4] {
    let (radius_px, width_px, intensity) = match ring {
        Some(r) => (r.radius_px, r.width_px, r.intensity),
        None => (0.0, 1.0, 0.0),
    };
    [
        radius_px,
        width_px.max(0.01),
        intensity,
        chromatic_aberration.max(0.0),
    ]
}

/// Linearly interpolates between two already-linear colors.
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Resamples an author-painted, arbitrary-length gradient (`FlashColor::HorizontalGradient`) onto
/// the fixed `FLASH_GRADIENT_STOPS`-stop internal representation every flash instance carries. An empty
/// list falls back to white (matching `ColorBinding::ByTrack`'s own empty-list fallback elsewhere
/// in this schema). Each stop is resolved against the triggering note's own velocity/pitch/track
/// (`resolve_for_note`), so a `ByVelocity`/`ByPitchClass`/`ByTrack` stop varies by which note
/// spawned this flash — the position of the stop within the gradient still comes from the
/// author's list order, only the color at each position is note-aware.
fn resample_gradient_stops(
    list: &[ColorBinding],
    velocity: u8,
    pitch: u8,
    track_id: usize,
) -> [[f32; 3]; FLASH_GRADIENT_STOPS] {
    if list.is_empty() {
        return [[1.0; 3]; FLASH_GRADIENT_STOPS];
    }
    let colors: Vec<[f32; 3]> = list
        .iter()
        .map(|c| srgb_to_linear(c.resolve_for_note(velocity, pitch, track_id)))
        .collect();
    let mut out = [[0.0f32; 3]; FLASH_GRADIENT_STOPS];
    for (i, stop) in out.iter_mut().enumerate() {
        let t = i as f32 / (FLASH_GRADIENT_STOPS - 1) as f32;
        let scaled = t * (colors.len() - 1) as f32;
        let i0 = (scaled.floor() as usize).min(colors.len() - 1);
        let i1 = (i0 + 1).min(colors.len() - 1);
        *stop = mix3(colors[i0], colors[i1], scaled - i0 as f32);
    }
    out
}

/// Cheap 2D value-noise hash (fract-based, no textures) — same formula as `barrier.wgsl`'s
/// `hash21`, ported to the CPU side for `flash_flicker` below. Pseudo-random but continuous.
fn hash21(x: f32, y: f32) -> f32 {
    let mut p3 = [
        (x * 0.1031).fract(),
        (y * 0.1030).fract(),
        (x * 0.0973).fract(),
    ];
    let d = p3[0] * (p3[1] + 33.33) + p3[1] * (p3[2] + 33.33) + p3[2] * (p3[0] + 33.33);
    for v in &mut p3 {
        *v += d;
    }
    ((p3[0] + p3[1]) * p3[2]).fract()
}

/// Bilinearly-interpolated (smoothstepped) value noise, range [-1, 1] — CPU port of
/// `barrier.wgsl`'s `noise2`, used below by `flash_flicker` instead of a literal sine wave so a
/// flicker reads as genuinely irregular rather than periodic.
fn noise2(x: f32, y: f32) -> f32 {
    let ix = x.floor();
    let iy = y.floor();
    let fx = x - ix;
    let fy = y - iy;
    let a = hash21(ix, iy);
    let b = hash21(ix + 1.0, iy);
    let c = hash21(ix, iy + 1.0);
    let d = hash21(ix + 1.0, iy + 1.0);
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uy = fy * fy * (3.0 - 2.0 * fy);
    let mix_ab = a + (b - a) * ux;
    let mix_cd = c + (d - c) * ux;
    (mix_ab + (mix_cd - mix_ab) * uy) * 2.0 - 1.0
}

/// A `FlashSpec::flicker_speed`/`flicker_intensity`-driven brightness multiplier in `[0, 1]` for a
/// sustained flash, sampled fresh every frame from the current transport time — same value-noise
/// approach (and the same `pow(.., 1.4)` contrast shaping) as `barrier.wgsl`'s per-strand flicker,
/// ported here since a flash's alpha is computed CPU-side rather than in a fragment shader.
/// `seed` decorrelates simultaneous flashes (each spawned flash gets its own random seed — see
/// `EffectsRenderer::spawn_flash`) so they don't flicker in lockstep. `speed <= 0.0` still returns
/// a well-defined (just time-invariant) value; callers gate the whole effect on `intensity` instead.
fn flash_flicker(seed: f32, time_seconds: f32, speed: f32) -> f32 {
    let n = noise2(seed * 7.0, time_seconds * speed + seed * 3.0);
    (n * 0.5 + 0.5).clamp(0.0, 1.0).powf(1.4)
}

/// Tiny deterministic xorshift32 PRNG for particle spawn jitter — not worth pulling in a `rand`
/// dependency (not used anywhere else in this workspace) for what only needs to look plausibly
/// random, not be cryptographically or statistically rigorous.
struct Rng(u32);

impl Rng {
    fn new() -> Self {
        Self(0x9E3779B9)
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn range(&mut self, low: f32, high: f32) -> f32 {
        let unit = self.next_u32() as f32 / u32::MAX as f32;
        low + (high - low) * unit
    }
}

pub struct EffectsRenderer {
    additive_pipeline: wgpu::RenderPipeline,
    alpha_pipeline: wgpu::RenderPipeline,

    quad_vertex_buffer: wgpu::Buffer,
    quad_index_buffer: wgpu::Buffer,
    quad_index_count: u32,

    view_buffer: wgpu::Buffer,
    view_bind_group: wgpu::BindGroup,

    additive_instances: Vec<EffectInstance>,
    additive_buffer: wgpu::Buffer,
    additive_capacity: usize,

    alpha_instances: Vec<EffectInstance>,
    alpha_buffer: wgpu::Buffer,
    alpha_capacity: usize,

    particles: Vec<Particle>,
    flashes: Vec<Flash>,
    rng: Rng,
    last_time_seconds: Option<f32>,
    canvas_size: (f32, f32),
}

impl EffectsRenderer {
    const INITIAL_INSTANCE_CAPACITY: usize = 256;

    pub fn new(device: &wgpu::Device, texture_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("effects.wgsl").into()),
        });

        let view_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("effects_view_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    // `VERTEX` alone was enough before Phase V — `fs_glow` now also reads
                    // `view_uniform.time.x` directly (the god-ray pulse/flicker/rotation noise is
                    // per-pixel, so it can't be baked into a vertex-stage-only value), so the
                    // fragment stage needs visibility into this binding too.
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let view_data = ViewUniform::default();
        let view_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("effects_view_uniform"),
            contents: bytemuck::cast_slice(&[view_data]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let view_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effects_view_bind_group"),
            layout: &view_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: view_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effects_pipeline_layout"),
            bind_group_layouts: &[Some(&view_bind_group_layout)],
            immediate_size: 0,
        });

        let quad_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        };
        let instance_attributes = EffectInstance::attributes();
        let instance_layout = EffectInstance::layout(&instance_attributes);

        // Additive (`One, One`) for flashes and additive-mode particles (sparks, glints — light
        // stacking on light, `fs_glow`'s additive-layered-sum formula); premultiplied-alpha
        // (`One, OneMinusSrcAlpha`) for particles with `ParticleSpec::additive = false`
        // (soft/smoke-like puffs that should occlude, not just brighten — `fs_puff`'s unchanged
        // hard-edge shape). Each pipeline uses a different fragment entry point.
        let additive_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let premultiplied_alpha_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };

        let make_pipeline = |label: &str, entry_point: &'static str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[quad_vertex_layout.clone(), instance_layout.clone()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: texture_format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let additive_pipeline =
            make_pipeline("effects_additive_pipeline", "fs_glow", additive_blend);
        let alpha_pipeline = make_pipeline(
            "effects_alpha_pipeline",
            "fs_puff",
            premultiplied_alpha_blend,
        );

        const VERTICES: &[QuadVertex] = &[
            QuadVertex {
                position: [0.0, 0.0],
            },
            QuadVertex {
                position: [1.0, 0.0],
            },
            QuadVertex {
                position: [1.0, 1.0],
            },
            QuadVertex {
                position: [0.0, 1.0],
            },
        ];
        #[rustfmt::skip]
        const INDICES: &[u16] = &[
            0, 1, 2,
            0, 2, 3,
        ];
        let quad_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("effects_quad_vertex_buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let quad_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("effects_quad_index_buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        let additive_buffer = Self::create_instance_buffer(device, Self::INITIAL_INSTANCE_CAPACITY);
        let alpha_buffer = Self::create_instance_buffer(device, Self::INITIAL_INSTANCE_CAPACITY);

        Self {
            additive_pipeline,
            alpha_pipeline,
            quad_vertex_buffer,
            quad_index_buffer,
            quad_index_count: INDICES.len() as u32,
            view_buffer,
            view_bind_group,
            additive_instances: Vec::new(),
            additive_buffer,
            additive_capacity: Self::INITIAL_INSTANCE_CAPACITY,
            alpha_instances: Vec::new(),
            alpha_buffer,
            alpha_capacity: Self::INITIAL_INSTANCE_CAPACITY,
            particles: Vec::new(),
            flashes: Vec::new(),
            rng: Rng::new(),
            last_time_seconds: None,
            canvas_size: (1.0, 1.0),
        }
    }

    fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effects_instance_buffer"),
            size: (std::mem::size_of::<EffectInstance>() * capacity.max(1)) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Advances the simulation to `time_seconds` (expected to already have the sync offset
    /// subtracted, same convention as `notes::NotesRenderer::update`/`barrier::BarrierRenderer::
    /// update_pulse`), spawning a one-shot burst/flash for every `note_intervals` entry crossed
    /// since the previous call (plus, for `EmissionMode::Continuous`, streaming particles for
    /// every interval currently held), then uploads the current pool. `note_intervals` must be
    /// sorted ascending by `start_seconds` (guaranteed by `notes::NotesRenderer::
    /// rebuild_instances`).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        canvas_size: (f32, f32),
        barrier_fraction: f32,
        transition_layer: &TransitionLayer,
        time_seconds: f32,
        note_intervals: &[NoteInterval],
    ) {
        self.canvas_size = (canvas_size.0.max(1.0), canvas_size.1.max(1.0));
        let barrier_y = self.canvas_size.1 * barrier_fraction;

        let step = self
            .last_time_seconds
            .map(|last| time_seconds - last)
            .unwrap_or(0.0);
        let is_ordinary_step =
            self.last_time_seconds.is_some() && (0.0..=MAX_ORDINARY_STEP_SECONDS).contains(&step);

        if is_ordinary_step {
            let last = self.last_time_seconds.unwrap();
            let start = note_intervals.partition_point(|e| e.start_seconds <= last);
            let end = note_intervals.partition_point(|e| e.start_seconds <= time_seconds);
            let spawn_particles = matches!(
                transition_layer.kind,
                TransitionKind::Particles | TransitionKind::ParticlesAndFlash
            );
            let spawn_flash = matches!(
                transition_layer.kind,
                TransitionKind::Flash | TransitionKind::ParticlesAndFlash
            );
            for interval in &note_intervals[start..end] {
                if spawn_particles {
                    if let Some(spec) = &transition_layer.particles {
                        if matches!(spec.emission, EmissionMode::Burst) {
                            self.spawn_particles(interval, time_seconds, barrier_y, spec);
                        }
                    }
                }
                if spawn_flash {
                    if let Some(spec) = &transition_layer.flash {
                        let decay_start = match spec.mode {
                            FlashMode::Instant => time_seconds,
                            FlashMode::Sustained => interval.end_seconds,
                        };
                        self.spawn_flash(interval, barrier_y, spec, decay_start);
                    }
                }
            }

            let dt = step.max(0.0);

            // Continuous emission: sample every note currently held (a plain "is this note
            // active right now" point check, not a crossing check like the burst spawn above —
            // this is a per-frame density sample, not a one-shot cue, so there's nothing to
            // "miss" the way a burst would if it skipped an arrival).
            if spawn_particles {
                if let Some(spec) = &transition_layer.particles {
                    if let EmissionMode::Continuous { rate_per_second } = spec.emission {
                        let expected = rate_per_second.max(0.0) * dt;
                        for interval in note_intervals {
                            if interval.start_seconds <= time_seconds
                                && time_seconds <= interval.end_seconds
                            {
                                // Color doesn't depend on spawn x (see `resolve_particle_color`),
                                // so it's resolved once per held note per frame rather than once
                                // per spawned particle — re-resolved fresh each frame from the
                                // current `time_seconds`, which is what makes `ParticleColor::
                                // MatchNote` slide across the note's own gradient as it's held.
                                let (color, y_gradient) = Self::resolve_particle_color(
                                    &spec.color,
                                    interval,
                                    time_seconds,
                                    barrier_y,
                                    self.canvas_size.1,
                                );
                                let brightness = spec.brightness.resolve_for_note(
                                    interval.velocity,
                                    interval.pitch,
                                    interval.track_id,
                                );
                                let shape = Self::resolve_particle_shape(spec, interval);
                                let mut n = expected.floor() as u32;
                                if self.rng.range(0.0, 1.0) < expected.fract() {
                                    n += 1;
                                }
                                for _ in 0..n {
                                    let x = self.rng.range(interval.x_left, interval.x_right);
                                    self.spawn_one_particle(
                                        x, barrier_y, spec, color, brightness, y_gradient, shape,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            for particle in &mut self.particles {
                particle.pos[0] += particle.vel[0] * dt;
                particle.pos[1] += particle.vel[1] * dt;
                particle.vel[1] += particle.gravity_px * dt;
                particle.life_seconds -= dt;
                if let Some(range) = particle.y_gradient {
                    let raw = range.color_at(particle.pos[1]);
                    particle.color = if particle.additive {
                        raw
                    } else {
                        hot_color(raw, particle.brightness)
                    };
                }
            }
            self.particles.retain(|p| p.life_seconds > 0.0);

            self.flashes
                .retain(|f| time_seconds - f.decay_start_seconds < f.decay_seconds);
        } else {
            // First call, or a scrub (forward or backward) large enough that "catch up" doesn't
            // make sense for a transient effect — clear rather than guess.
            self.particles.clear();
            self.flashes.clear();
        }
        self.last_time_seconds = Some(time_seconds);

        let particle_additive = transition_layer
            .particles
            .as_ref()
            .map(|spec| spec.additive)
            .unwrap_or(true);
        self.rebuild_instances(particle_additive, time_seconds);

        queue.write_buffer(
            &self.view_buffer,
            0,
            bytemuck::cast_slice(&[ViewUniform {
                transform: orthographic_projection(self.canvas_size.0, self.canvas_size.1),
                time: [time_seconds, 0.0, 0.0, 0.0],
            }]),
        );
        self.upload(device, queue);
    }

    /// Resolves a particle's spawn color, per `ParticleColor` (the single mutually-exclusive mode
    /// selector — see that enum's doc comment). Doesn't depend on the particle's spawn x at all —
    /// `MatchNote` is `interval.color_at_barrier(time_seconds)`, whichever part of the note is
    /// currently crossing the barrier, not a finer per-pixel sample of it (see that method's doc
    /// comment for why) — so this can be (and is) called once per note per frame and reused for
    /// every particle spawned from it, rather than once per particle. Under continuous emission
    /// this is exactly what makes a held note's particle stream slide across the note's own
    /// gradient over time instead of staying pinned to the color it had on arrival. `Fixed`/
    /// `MatchNote` return `None` (nothing left to recompute after spawn); `YGradient` returns the
    /// color at the *current* `y_px` plus `Some(range)` so the caller's `Particle::y_gradient` can
    /// keep re-evaluating it as the particle moves — `canvas_height` scales `top_fraction`/
    /// `bottom_fraction` into the pixel span that range blends across.
    fn resolve_particle_color(
        color: &ParticleColor,
        interval: &NoteInterval,
        time_seconds: f32,
        y_px: f32,
        canvas_height: f32,
    ) -> ([f32; 3], Option<YGradientRange>) {
        match color {
            ParticleColor::Fixed(binding) => (
                srgb_to_linear(binding.resolve_for_note(
                    interval.velocity,
                    interval.pitch,
                    interval.track_id,
                )),
                None,
            ),
            ParticleColor::MatchNote => (interval.color_at_barrier(time_seconds), None),
            ParticleColor::YGradient {
                top,
                bottom,
                top_fraction,
                bottom_fraction,
            } => {
                let range = YGradientRange {
                    top: srgb_to_linear(top.resolve_for_note(
                        interval.velocity,
                        interval.pitch,
                        interval.track_id,
                    )),
                    bottom: srgb_to_linear(bottom.resolve_for_note(
                        interval.velocity,
                        interval.pitch,
                        interval.track_id,
                    )),
                    top_px: canvas_height * top_fraction,
                    bottom_px: canvas_height * bottom_fraction,
                };
                (range.color_at(y_px), Some(range))
            }
        }
    }

    /// Resolves `ParticleSpec`'s five per-note-varying shape scalars (`lifetime_seconds`/
    /// `size_px`/`speed_px`/`spread_degrees`/`gravity_px`) against one triggering/held note in one
    /// call — every spawn site (`spawn_particles`, continuous emission) needs all five together,
    /// same mechanism (`ScalarBinding::resolve_for_note`) as `brightness`/`color` above.
    fn resolve_particle_shape(
        spec: &ParticleSpec,
        interval: &NoteInterval,
    ) -> (f32, f32, f32, f32, f32) {
        (
            spec.lifetime_seconds.resolve_for_note(
                interval.velocity,
                interval.pitch,
                interval.track_id,
            ),
            spec.size_px
                .resolve_for_note(interval.velocity, interval.pitch, interval.track_id),
            spec.speed_px
                .resolve_for_note(interval.velocity, interval.pitch, interval.track_id),
            spec.spread_degrees.resolve_for_note(
                interval.velocity,
                interval.pitch,
                interval.track_id,
            ),
            spec.gravity_px
                .resolve_for_note(interval.velocity, interval.pitch, interval.track_id),
        )
    }

    fn spawn_particles(
        &mut self,
        interval: &NoteInterval,
        time_seconds: f32,
        barrier_y: f32,
        spec: &ParticleSpec,
    ) {
        let (color, y_gradient) = Self::resolve_particle_color(
            &spec.color,
            interval,
            time_seconds,
            barrier_y,
            self.canvas_size.1,
        );
        let brightness =
            spec.brightness
                .resolve_for_note(interval.velocity, interval.pitch, interval.track_id);
        let shape = Self::resolve_particle_shape(spec, interval);
        for _ in 0..spec.count {
            self.spawn_one_particle(
                interval.x_center(),
                barrier_y,
                spec,
                color,
                brightness,
                y_gradient,
                shape,
            );
        }
    }

    /// Spawns a single particle — shared by burst spawns (`spawn_particles`, called `count`
    /// times per note arrival) and continuous emission (called a variable number of times per
    /// frame, spread across a held note's key width). `color` is the already-resolved (linear)
    /// spawn color; `brightness` is `spec.brightness` already resolved (`ScalarBinding::
    /// resolve_for_note`) against the same triggering/held note `color` was resolved against —
    /// both callers resolve once per note (not once per particle) and pass the result in, rather
    /// than re-resolving per spawned particle, since neither varies within one note's burst.
    /// `shape` is `resolve_particle_shape`'s tuple, resolved the same way and for the same reason.
    /// Non-additive "puff" particles get `hot_color`-whitened fill color (no corona); additive
    /// particles instead leave `color` unwhitened and pre-multiply `brightness` into `layer_amp`,
    /// since their corona is additive and whitens via saturation instead. `y_gradient`, if `Some`,
    /// is stored on the particle so `update`'s per-step loop can keep recomputing `color` as the
    /// particle moves (see `ParticleColor::YGradient`).
    #[allow(clippy::too_many_arguments)]
    fn spawn_one_particle(
        &mut self,
        x_px: f32,
        y_px: f32,
        spec: &ParticleSpec,
        color: [f32; 3],
        brightness: f32,
        y_gradient: Option<YGradientRange>,
        shape: (f32, f32, f32, f32, f32),
    ) {
        let (lifetime_seconds, size_px, speed_px, spread_degrees, gravity_px) = shape;
        let (color, margin_px, layer_amp, layer_sigma) = if spec.additive {
            let layer_amp = [
                spec.layers[0].amplitude * brightness,
                spec.layers[1].amplitude * brightness,
                spec.layers[2].amplitude * brightness,
            ];
            let layer_sigma = [
                spec.layers[0].sigma_px,
                spec.layers[1].sigma_px,
                spec.layers[2].sigma_px,
            ];
            let margin_px = spec
                .layers
                .iter()
                .fold(0.0f32, |acc, layer| acc.max(layer.sigma_px))
                * GLOW_CUTOFF_SIGMAS;
            (color, margin_px, layer_amp, layer_sigma)
        } else {
            (hot_color(color, brightness), 0.0, [0.0; 3], [1.0; 3])
        };
        // Spread around straight up (canvas convention is y-down, so "up" is negative y) —
        // `angle_degrees` measured from that upward axis.
        let angle_deg = -90.0 + self.rng.range(-spread_degrees * 0.5, spread_degrees * 0.5);
        let angle = angle_deg.to_radians();
        let speed = speed_px * self.rng.range(0.5, 1.0);
        let lifetime = lifetime_seconds.max(0.01);
        self.particles.push(Particle {
            pos: [x_px, y_px],
            vel: [angle.cos() * speed, angle.sin() * speed],
            gravity_px,
            life_seconds: lifetime,
            lifetime_seconds: lifetime,
            size_px: size_px.max(0.5),
            color,
            y_gradient,
            brightness,
            additive: spec.additive,
            margin_px,
            layer_amp,
            layer_sigma,
        });
    }

    fn spawn_flash(
        &mut self,
        interval: &NoteInterval,
        y_px: f32,
        spec: &FlashSpec,
        decay_start_seconds: f32,
    ) {
        let x_px = interval.x_center();
        // A flash always renders additively: color stops stay unwhitened, `brightness`
        // is pre-multiplied into `layer_amp` instead of baked in via `hot_color` — additive
        // saturation whitens for free. A flash is always fully "on" at spawn, fading to 0 over
        // `decay_seconds`.
        let color = match &spec.color {
            FlashColor::Solid(binding) => {
                let resolved = srgb_to_linear(binding.resolve_for_note(
                    interval.velocity,
                    interval.pitch,
                    interval.track_id,
                ));
                FlashColorSource::Fixed([resolved; FLASH_GRADIENT_STOPS])
            }
            FlashColor::HorizontalGradient(list) => FlashColorSource::Fixed(
                resample_gradient_stops(list, interval.velocity, interval.pitch, interval.track_id),
            ),
            // Kept as the source `NoteInterval` rather than baked to a color now — see
            // `FlashColorSource::MatchNote`'s doc comment for why this needs re-evaluating every
            // frame instead of once at spawn.
            FlashColor::MatchNote => FlashColorSource::MatchNote(*interval),
        };
        let brightness =
            spec.brightness
                .resolve_for_note(interval.velocity, interval.pitch, interval.track_id);
        let decay_seconds = spec.decay_seconds.resolve_for_note(
            interval.velocity,
            interval.pitch,
            interval.track_id,
        );
        let radius_x_px =
            spec.radius_x_px
                .resolve_for_note(interval.velocity, interval.pitch, interval.track_id);
        let radius_y_px =
            spec.radius_y_px
                .resolve_for_note(interval.velocity, interval.pitch, interval.track_id);
        let flicker_speed = spec.flicker_speed.resolve_for_note(
            interval.velocity,
            interval.pitch,
            interval.track_id,
        );
        let flicker_intensity = spec.flicker_intensity.resolve_for_note(
            interval.velocity,
            interval.pitch,
            interval.track_id,
        );
        let flicker_seed = self.rng.range(0.0, 1000.0);
        let layer_amp = [
            spec.layers[0].amplitude * brightness,
            spec.layers[1].amplitude * brightness,
            spec.layers[2].amplitude * brightness,
        ];
        let layer_sigma = [
            spec.layers[0].sigma_px,
            spec.layers[1].sigma_px,
            spec.layers[2].sigma_px,
        ];
        let (godray_a, godray_b, godray_c) = resolve_god_ray_params(spec.god_rays.as_ref());
        let ring_chromatic =
            resolve_ring_chromatic_params(spec.ring.as_ref(), spec.chromatic_aberration);
        // The quad must reach as far as the farthest-visible effect, not just the corona layers —
        // a god ray's own length (`godray_a[1]`, before the `* 1.5` jitter headroom and `* 1.15`
        // outer-cut falloff in `effects.wgsl`'s `god_ray_strength`) or the ring's outer edge
        // (`ring_chromatic[0] + ring_chromatic[1]`) can both reach well past the base corona,
        // otherwise the quad edge would visibly clip the rays/ring.
        let corona_margin_px = spec
            .layers
            .iter()
            .fold(0.0f32, |acc, layer| acc.max(layer.sigma_px))
            * GLOW_CUTOFF_SIGMAS;
        let god_ray_margin_px = godray_a[1] * 2.0;
        let ring_margin_px = ring_chromatic[0] + ring_chromatic[1];
        let margin_px = corona_margin_px.max(god_ray_margin_px).max(ring_margin_px);
        self.flashes.push(Flash {
            pos: [x_px, y_px],
            decay_start_seconds,
            decay_seconds: decay_seconds.max(0.01),
            radius_x_px: radius_x_px.max(1.0),
            radius_y_px: radius_y_px.max(1.0),
            color,
            margin_px,
            layer_amp,
            layer_sigma,
            flicker_speed,
            flicker_intensity: flicker_intensity.clamp(0.0, 1.0),
            flicker_seed,
            godray_a,
            godray_b,
            godray_c,
            ring_chromatic,
        });
    }

    /// Rebuilds the two CPU-side instance lists from the current particle/flash pool state.
    /// Flashes always draw additive (a flash reads as a bright pop regardless of style); particles
    /// draw additive or premultiplied-alpha together, chosen by the *currently resolved*
    /// `ParticleSpec::additive` — a style swap mid-flight is a documented edge case where
    /// already-alive particles from the previous style render under the new blend mode instead of
    /// finishing out their old one, not worth extra per-particle bookkeeping for.
    fn rebuild_instances(&mut self, particle_additive: bool, time_seconds: f32) {
        self.additive_instances.clear();
        self.alpha_instances.clear();

        for flash in &self.flashes {
            // Pure function of current transport time vs. the flash's stored decay-start
            // threshold — for `FlashMode::Instant` that threshold is the spawn time, so `elapsed`
            // grows immediately; for `Sustained` it's the note's `end_seconds` (in the future at
            // spawn time), so `elapsed` stays clamped to 0 (t == 1.0, full intensity) for the
            // note's entire held duration and only starts decaying once the note actually ends.
            let elapsed = (time_seconds - flash.decay_start_seconds).max(0.0);
            let mut t = 1.0 - (elapsed / flash.decay_seconds).clamp(0.0, 1.0);
            // `flicker_intensity == 0.0` (the default) makes this an exact no-op: `1.0 - 0.0 +
            // 0.0 * flick == 1.0`. Otherwise dims `t` toward `1.0 - flicker_intensity` at the
            // flicker's trough and leaves it unchanged at its peak — see `flash_flicker`'s doc
            // comment for why this is most visible on a long-held `FlashMode::Sustained` flash.
            if flash.flicker_intensity > 0.0 {
                let flick = flash_flicker(flash.flicker_seed, time_seconds, flash.flicker_speed);
                t *= 1.0 - flash.flicker_intensity + flash.flicker_intensity * flick;
            }
            let core_radius = [flash.radius_x_px, flash.radius_y_px];
            // `Fixed` stops are unchanged from spawn; `MatchNote` is re-evaluated against the
            // current `time_seconds` every frame, so a `FlashMode::Sustained` flash keeps sliding
            // across the note's own gradient for as long as it stays lit (see
            // `FlashColorSource::MatchNote`'s doc comment).
            let color_stops = match &flash.color {
                FlashColorSource::Fixed(stops) => *stops,
                FlashColorSource::MatchNote(interval) => {
                    [interval.color_at_barrier(time_seconds); FLASH_GRADIENT_STOPS]
                }
            };
            self.additive_instances.push(EffectInstance {
                center: flash.pos,
                core_radius,
                quad_radius: [
                    core_radius[0] + flash.margin_px,
                    core_radius[1] + flash.margin_px,
                ],
                alpha: t,
                color_stops,
                layer_amp: flash.layer_amp,
                layer_sigma: flash.layer_sigma,
                godray_a: flash.godray_a,
                godray_b: flash.godray_b,
                godray_c: flash.godray_c,
                ring_chromatic: flash.ring_chromatic,
            });
        }

        let target = if particle_additive {
            &mut self.additive_instances
        } else {
            &mut self.alpha_instances
        };
        for particle in &self.particles {
            let t = (particle.life_seconds / particle.lifetime_seconds).clamp(0.0, 1.0);
            let core_radius = [particle.size_px, particle.size_px];
            target.push(EffectInstance {
                center: particle.pos,
                core_radius,
                quad_radius: [
                    core_radius[0] + particle.margin_px,
                    core_radius[1] + particle.margin_px,
                ],
                alpha: t,
                color_stops: [particle.color; FLASH_GRADIENT_STOPS],
                layer_amp: particle.layer_amp,
                layer_sigma: particle.layer_sigma,
                godray_a: ZERO_GOD_RAYS.0,
                godray_b: ZERO_GOD_RAYS.1,
                godray_c: ZERO_GOD_RAYS.2,
                ring_chromatic: ZERO_RING_CHROMATIC,
            });
        }
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.additive_instances.len() > self.additive_capacity {
            self.additive_capacity = self.additive_instances.len();
            self.additive_buffer = Self::create_instance_buffer(device, self.additive_capacity);
        }
        queue.write_buffer(
            &self.additive_buffer,
            0,
            bytemuck::cast_slice(&self.additive_instances),
        );

        if self.alpha_instances.len() > self.alpha_capacity {
            self.alpha_capacity = self.alpha_instances.len();
            self.alpha_buffer = Self::create_instance_buffer(device, self.alpha_capacity);
        }
        queue.write_buffer(
            &self.alpha_buffer,
            0,
            bytemuck::cast_slice(&self.alpha_instances),
        );
    }

    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        // `barrier::BarrierRenderer::render` (drawn just before this) already resets the scissor
        // rect to the full canvas, but reset again defensively rather than depend on draw order
        // elsewhere in `Compositor::render` never changing.
        render_pass.set_scissor_rect(
            0,
            0,
            self.canvas_size.0.round().max(1.0) as u32,
            self.canvas_size.1.round().max(1.0) as u32,
        );

        if !self.additive_instances.is_empty() {
            render_pass.set_pipeline(&self.additive_pipeline);
            render_pass.set_bind_group(0, &self.view_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.additive_buffer.slice(..));
            render_pass
                .set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(
                0..self.quad_index_count,
                0,
                0..self.additive_instances.len() as u32,
            );
        }

        if !self.alpha_instances.is_empty() {
            render_pass.set_pipeline(&self.alpha_pipeline);
            render_pass.set_bind_group(0, &self.view_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, self.alpha_buffer.slice(..));
            render_pass
                .set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            render_pass.draw_indexed(
                0..self.quad_index_count,
                0,
                0..self.alpha_instances.len() as u32,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_flicker_stays_in_unit_range_across_time_and_seeds() {
        for seed in [0.0, 1.0, 13.7, 250.0] {
            for i in 0..200 {
                let t = i as f32 * 0.1;
                let f = flash_flicker(seed, t, 2.0);
                assert!((0.0..=1.0).contains(&f), "seed={seed} t={t} flick={f}");
            }
        }
    }

    #[test]
    fn flash_flicker_is_not_perfectly_periodic() {
        // A literal sine wave would repeat exactly every period; value noise shouldn't, so two
        // widely separated samples at speed 1.0 (period-ish 1.0) should generally disagree.
        let a: Vec<f32> = (0..20).map(|i| flash_flicker(0.0, i as f32, 1.0)).collect();
        let all_equal = a.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-6);
        assert!(!all_equal, "flicker should vary across samples, got {a:?}");
    }

    #[test]
    fn flash_flicker_decorrelates_by_seed() {
        let a = flash_flicker(1.0, 5.0, 2.0);
        let b = flash_flicker(999.0, 5.0, 2.0);
        assert!(
            (a - b).abs() > 1e-4,
            "different seeds at the same time should generally differ, got a={a} b={b}"
        );
    }

    #[test]
    fn noise2_stays_in_signed_unit_range() {
        for i in 0..50 {
            let x = i as f32 * 0.37;
            let y = i as f32 * 1.13;
            let n = noise2(x, y);
            assert!((-1.0..=1.0).contains(&n), "x={x} y={y} n={n}");
        }
    }
}
