//! Throwaway generator for `examples/styles/*.fmstyle.ron` — builds each sample as real `Style`
//! values and serializes them, so the shipped files are guaranteed to match whatever RON syntax
//! this version of `ron` actually produces (rather than hand-typing RON that could drift from it).
//! Run with `cargo run -p project --example dump_sample_styles` and copy stdout sections into the
//! corresponding files if the schema ever changes.

use project::{
    BarrierLayer, ColorBinding, Fill, FlashSpec, Glow, GlowLayer, NoteLayer, ParticleSpec, Pulse,
    Sheen, Style, Timed, TransitionKind, TransitionLayer, WavySpec,
};

fn scaled_layers(old_radius_px: f32) -> [GlowLayer; 3] {
    let scale = old_radius_px / 48.0;
    [
        GlowLayer {
            amplitude: 2.6,
            sigma_px: 5.0 * scale,
        },
        GlowLayer {
            amplitude: 1.1,
            sigma_px: 16.0 * scale,
        },
        GlowLayer {
            amplitude: 0.38,
            sigma_px: 48.0 * scale,
        },
    ]
}

fn print_style(name: &str, style: &Style) {
    println!("=== {name} ===");
    println!(
        "{}",
        ron::ser::to_string_pretty(style, ron::ser::PrettyConfig::new()).unwrap()
    );
}

fn main() {
    let gradient_glow = Style {
        version: 1,
        notes: Timed::Static(NoteLayer {
            fill: Fill::VerticalGradient {
                top: ColorBinding::Constant([120, 220, 255]),
                bottom: ColorBinding::Constant([30, 90, 200]),
            },
            sheen: Some(Sheen {
                intensity: 0.6,
                width: 0.25,
                angle_degrees: 35.0,
            }),
            glow: Some(Glow {
                color: ColorBinding::Constant([120, 200, 255]),
                brightness: 1.0,
                layers: scaled_layers(12.0),
                edge_blend_px: 0.0,
            }),
            roundedness: 1.0,
            fall_speed: 400.0,
            border: None,
            black_key_fill: project::BlackKeyFill::Auto,
        }),
        barrier: Timed::Static(BarrierLayer::default()),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let barrier_pulse = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer {
            color: ColorBinding::Constant([255, 220, 120]),
            thickness: 6.0,
            glow: Some(Glow {
                color: ColorBinding::Constant([255, 220, 120]),
                brightness: 1.0,
                layers: scaled_layers(24.0),
                edge_blend_px: 0.0,
            }),
            pulse: Some(Pulse {
                decay_seconds: 0.35,
                brightness: 1.6,
            }),
            wavy: None,
            show_bar: true,
        }),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let barrier_wavy = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer {
            color: ColorBinding::Constant([120, 200, 255]),
            thickness: 5.0,
            glow: Some(Glow {
                color: ColorBinding::Constant([120, 200, 255]),
                brightness: 1.0,
                layers: scaled_layers(18.0),
                edge_blend_px: 0.0,
            }),
            pulse: None,
            wavy: Some(WavySpec {
                amplitude_px: 6.0,
                wavelength_px: 220.0,
                speed: 18.0,
                mode: project::WavyMode::FullWave,
            }),
            show_bar: true,
        }),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let sparks = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer::default()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::ParticlesAndFlash,
            particles: Some(ParticleSpec {
                count: 24,
                lifetime_seconds: 0.4,
                size_px: 4.0,
                speed_px: 180.0,
                spread_degrees: 60.0,
                gravity_px: 300.0,
                color: ColorBinding::Constant([255, 240, 200]),
                additive: true,
                emission: project::EmissionMode::Burst,
                brightness: 1.0,
                layers: scaled_layers(4.0),
            }),
            flash: Some(FlashSpec {
                radius_x_px: 40.0,
                radius_y_px: 40.0,
                color: ColorBinding::Constant([255, 255, 255]),
                decay_seconds: 0.15,
                mode: project::FlashMode::Instant,
                brightness: 1.0,
                layers: scaled_layers(40.0),
            }),
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let ellipse_flash = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer::default()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::Flash,
            particles: None,
            flash: Some(FlashSpec {
                radius_x_px: 70.0,
                radius_y_px: 20.0,
                color: ColorBinding::Constant([255, 255, 255]),
                decay_seconds: 0.2,
                mode: project::FlashMode::Instant,
                brightness: 1.0,
                layers: scaled_layers(45.0),
            }),
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let grinding_particles = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer::default()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::Particles,
            particles: Some(ParticleSpec {
                count: 0,
                lifetime_seconds: 0.45,
                size_px: 6.0,
                speed_px: 140.0,
                spread_degrees: 100.0,
                gravity_px: 250.0,
                color: ColorBinding::Constant([255, 210, 90]),
                additive: true,
                emission: project::EmissionMode::Continuous {
                    rate_per_second: 30.0,
                },
                brightness: 1.0,
                layers: scaled_layers(6.0),
            }),
            flash: None,
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let key_glow = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer::default()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::Flash,
            particles: None,
            flash: Some(FlashSpec {
                radius_x_px: 50.0,
                radius_y_px: 30.0,
                color: ColorBinding::Constant([255, 220, 140]),
                decay_seconds: 0.6,
                mode: project::FlashMode::Sustained,
                brightness: 1.0,
                layers: scaled_layers(40.0),
            }),
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // Phase N: `background` is the one field being demonstrated here — everything else stays at
    // (or close to) its default so the dark-navy canvas color, not any other effect, is what
    // reads as the point of this sample. `show_bar: false` on a modest glow keeps the barrier from
    // reading as a plain white line against the new background, without adding an unrelated look.
    let dark_background = Style {
        version: 1,
        notes: Timed::Static(NoteLayer {
            fill: Fill::Solid(ColorBinding::Constant([255, 200, 90])),
            ..NoteLayer::default()
        }),
        barrier: Timed::Static(BarrierLayer {
            color: ColorBinding::Constant([120, 200, 255]),
            glow: Some(Glow {
                color: ColorBinding::Constant([120, 200, 255]),
                brightness: 1.4,
                layers: scaled_layers(24.0),
                edge_blend_px: 0.0,
            }),
            show_bar: false,
            ..BarrierLayer::default()
        }),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([8, 10, 24]),
    };

    print_style("gradient-glow", &gradient_glow);
    print_style("barrier-pulse", &barrier_pulse);
    print_style("barrier-wavy", &barrier_wavy);
    print_style("sparks", &sparks);
    print_style("ellipse-flash", &ellipse_flash);
    print_style("grinding-particles", &grinding_particles);
    print_style("key-glow", &key_glow);
    print_style("dark-background", &dark_background);
}
