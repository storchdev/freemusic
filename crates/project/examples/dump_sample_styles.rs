//! Throwaway generator for `examples/styles/*.fmstyle.ron` — builds each sample as real `Style`
//! values and serializes them, so the shipped files are guaranteed to match whatever RON syntax
//! this version of `ron` actually produces (rather than hand-typing RON that could drift from it).
//! Run with `cargo run -p project --example dump_sample_styles` and copy stdout sections into the
//! corresponding files if the schema ever changes.

use project::{
    BarrierLayer, ColorBinding, Fill, FlashSpec, Glow, NoteLayer, ParticleSpec, Pulse, Sheen,
    Style, Timed, TransitionKind, TransitionLayer, WavySpec,
};

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
                radius_px: 12.0,
                brightness: 1.0,
            }),
            roundedness: 1.0,
            fall_speed: 400.0,
            border: None,
            black_key_fill: project::BlackKeyFill::Auto,
        }),
        barrier: Timed::Static(BarrierLayer::default()),
        transition: Timed::Static(TransitionLayer::default()),
    };

    let barrier_pulse = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer {
            color: ColorBinding::Constant([255, 220, 120]),
            thickness: 6.0,
            glow: Some(Glow {
                color: ColorBinding::Constant([255, 220, 120]),
                radius_px: 24.0,
                brightness: 1.0,
            }),
            pulse: Some(Pulse {
                decay_seconds: 0.35,
                brightness: 1.6,
            }),
            wavy: None,
        }),
        transition: Timed::Static(TransitionLayer::default()),
    };

    let barrier_wavy = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer {
            color: ColorBinding::Constant([120, 200, 255]),
            thickness: 5.0,
            glow: Some(Glow {
                color: ColorBinding::Constant([120, 200, 255]),
                radius_px: 18.0,
                brightness: 1.0,
            }),
            pulse: None,
            wavy: Some(WavySpec {
                amplitude_px: 6.0,
                wavelength_px: 220.0,
                speed: 18.0,
                mode: project::WavyMode::FullWave,
            }),
        }),
        transition: Timed::Static(TransitionLayer::default()),
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
            }),
            flash: Some(FlashSpec {
                radius_x_px: 40.0,
                radius_y_px: 40.0,
                color: ColorBinding::Constant([255, 255, 255]),
                decay_seconds: 0.15,
                mode: project::FlashMode::Instant,
                brightness: 1.0,
            }),
        }),
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
            }),
        }),
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
            }),
            flash: None,
        }),
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
            }),
        }),
    };

    print_style("gradient-glow", &gradient_glow);
    print_style("barrier-pulse", &barrier_pulse);
    print_style("barrier-wavy", &barrier_wavy);
    print_style("sparks", &sparks);
    print_style("ellipse-flash", &ellipse_flash);
    print_style("grinding-particles", &grinding_particles);
    print_style("key-glow", &key_glow);
}
