//! Throwaway generator for `examples/styles/*.fmstyle.ron` — builds each sample as real `Style`
//! values and serializes them, so the shipped files are guaranteed to match whatever RON syntax
//! this version of `ron` actually produces (rather than hand-typing RON that could drift from it).
//! Run with `cargo run -p project --example dump_sample_styles` and copy stdout sections into the
//! corresponding files if the schema ever changes.

use project::{
    BarrierKind, BarrierLayer, ColorBinding, Fill, FlashSpec, Glow, NoteLayer, ParticleSpec, Pulse,
    Sheen, Style, Timed, TransitionKind, TransitionLayer,
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
                intensity: 0.5,
            }),
            roundedness: 1.0,
            fall_speed: 400.0,
            border: None,
        }),
        barrier: Timed::Static(BarrierLayer::default()),
        transition: Timed::Static(TransitionLayer::default()),
    };

    let barrier_pulse = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer {
            kind: BarrierKind::Glow,
            color: ColorBinding::Constant([255, 220, 120]),
            thickness: 6.0,
            glow_radius_px: 24.0,
            pulse: Some(Pulse {
                intensity: 0.8,
                decay_seconds: 0.35,
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
            }),
            flash: Some(FlashSpec {
                radius_px: 40.0,
                intensity: 0.9,
                color: ColorBinding::Constant([255, 255, 255]),
                decay_seconds: 0.15,
            }),
        }),
    };

    print_style("gradient-glow", &gradient_glow);
    print_style("barrier-pulse", &barrier_pulse);
    print_style("sparks", &sparks);
}
