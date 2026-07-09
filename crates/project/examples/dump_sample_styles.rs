//! Throwaway generator for `examples/styles/*.fmstyle.ron` — builds each sample as real `Style`
//! values and serializes them, so the shipped files are guaranteed to match whatever RON syntax
//! this version of `ron` actually produces (rather than hand-typing RON that could drift from it).
//! Run with `cargo run -p project --example dump_sample_styles` and copy stdout sections into the
//! corresponding files if the schema ever changes.

use project::{
    BarrierLayer, BlackKeyFill, ColorBinding, Fill, FlashMode, FlashSpec, Glow, GlowLayer,
    NoteLayer, ParticleSpec, Pulse, Sheen, StrandSpec, Style, Timed, TransitionKind,
    TransitionLayer, WavyMode, WavySpec,
};

fn glow_layers(tight: f32, mid: f32, wide: f32) -> [GlowLayer; 3] {
    [
        GlowLayer {
            amplitude: 2.6,
            sigma_px: tight,
        },
        GlowLayer {
            amplitude: 1.1,
            sigma_px: mid,
        },
        GlowLayer {
            amplitude: 0.38,
            sigma_px: wide,
        },
    ]
}

fn hot_layers(tight: f32, mid: f32, wide: f32) -> [GlowLayer; 3] {
    [
        GlowLayer {
            amplitude: 3.0,
            sigma_px: tight,
        },
        GlowLayer {
            amplitude: 2.0,
            sigma_px: mid,
        },
        GlowLayer {
            amplitude: 0.85,
            sigma_px: wide,
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

fn visible_barrier() -> BarrierLayer {
    BarrierLayer {
        show_bar: true,
        ..BarrierLayer::default()
    }
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
                intensity: 0.5,
                width: 0.8,
                angle_degrees: 45.0,
            }),
            glow: Some(Glow {
                color: ColorBinding::Constant([120, 200, 255]),
                brightness: 0.8,
                layers: glow_layers(2.0, 4.0, 10.0),
                edge_blend_px: 6.0,
            }),
            roundedness: 1.0,
            fall_speed: 400.0,
            border: None,
            black_key_fill: BlackKeyFill::Auto,
        }),
        barrier: Timed::Static(visible_barrier()),
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
                brightness: 1.5,
                layers: hot_layers(2.0, 4.0, 8.0),
                edge_blend_px: 0.0,
            }),
            pulse: Some(Pulse {
                decay_seconds: 0.35,
                brightness: 1.6,
            }),
            wavy: None,
            show_bar: false,
        }),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let barrier_wavy = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer {
            color: ColorBinding::Constant([120, 200, 255]),
            thickness: 4.0,
            glow: Some(Glow {
                color: ColorBinding::Constant([120, 200, 255]),
                brightness: 1.5,
                layers: hot_layers(2.0, 4.0, 8.0),
                edge_blend_px: 0.0,
            }),
            pulse: None,
            wavy: Some(WavySpec {
                amplitude_px: 10.0,
                wavelength_px: 50.0,
                speed: 2.0,
                mode: WavyMode::Edge,
                slide_speed: 0.0,
                strands: None,
            }),
            show_bar: false,
        }),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let barrier_wavy_volume = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer {
            color: ColorBinding::Constant([120, 200, 255]),
            thickness: 5.0,
            glow: Some(Glow {
                color: ColorBinding::Constant([120, 200, 255]),
                brightness: 1.4,
                layers: hot_layers(2.0, 4.0, 8.0),
                edge_blend_px: 0.0,
            }),
            pulse: None,
            wavy: Some(WavySpec {
                amplitude_px: 6.0,
                wavelength_px: 50.0,
                speed: 18.0,
                mode: WavyMode::FullWave,
                slide_speed: 0.0,
                strands: None,
            }),
            show_bar: false,
        }),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // Phase O: the strand bundle ported from `explorations/barrier-fx-lab` — several thin,
    // independently-flickering filament threads fraying off the wavy top edge, rather than one
    // smooth wavy line. `mode: Edge` is required for strands to render at all (see `StrandSpec`'s
    // doc comment). Values here are the `explorations/barrier-fx-lab/presets/seemusic-found.json`
    // preset (the closest match found so far to the real SeeMusic edge, see `sm-ex.png`),
    // translated field-by-field into this schema, with one deliberate deviation (`pulse: None`,
    // see below):
    // - `coreColor`/`glowColor` hex -> `color`/`glow.color` RGB.
    // - `brightnessBase` -> `glow.brightness` directly (resting brightness, same meaning in both
    //   models). `brightnessPeak`/`pulseDecay` (the lab's note-arrival pulse) are **deliberately
    //   not carried over** — `pulse: None` — so the barrier holds steady at its resting glow
    //   instead of periodically brightening on each note; this is the one place this sample
    //   departs from a literal 1:1 translation of the preset.
    // - `layerAmpN` * `glowIntensity` -> `layers[N].amplitude` (this schema has no separate
    //   intensity knob distinct from `Glow::brightness`, so the lab's multiplier is baked
    //   directly into each layer's amplitude instead); `layerSigmaN` -> `layers[N].sigma_px`
    //   unchanged (`glowSizeScale` was `1`, a no-op).
    // - `waveAmp`/`waveLen`/`waveSpeed`/`slideSpeed`/`wavyMode` -> `amplitude_px`/`wavelength_px`/
    //   `speed`/`slide_speed`/`mode` directly (`slide_speed` ported after this phase originally
    //   shipped — see `WavySpec::slide_speed`'s own doc comment for how it differs from `speed`).
    // - `strandCount`/`strandSpread`/`strandJitter`/`strandThickness`/`strandHaloAmp`/
    //   `strandHaloSigma`/`strandGlow`/`strandFlicker` -> `StrandSpec`'s identically-purposed
    //   fields, 1:1, no translation needed. Several of these (`spread_px`/`thickness_px`/
    //   `glow_intensity`, and the barrier's own `thickness`) have since been hand-tuned away from
    //   a literal preset translation — see this style's own values below, not the preset JSON, for
    //   the current look.
    // - `filamentIntensity`/`filamentSpeed`/`filamentScale` (sliding filament) and
    //   `wispDensity`/`wispHeight`/`wispFlicker`/`wispSway`/`wispIntensity` (wisps) have no
    //   real-app equivalent — out of scope for this phase, per its own doc comment.
    // - `barrierYFrac`/`keyWidth`/`vignette`/`exposure` are lab-scene-only (mock piano/vignette/
    //   tonemapping for the standalone preview canvas), not part of this schema at all.
    let barrier_strands = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(BarrierLayer {
            color: ColorBinding::Constant([255, 180, 84]),
            thickness: 0.0,
            glow: Some(Glow {
                color: ColorBinding::Constant([255, 217, 160]),
                brightness: 1.0,
                layers: [
                    GlowLayer {
                        amplitude: 0.988,
                        sigma_px: 5.0,
                    },
                    GlowLayer {
                        amplitude: 0.418,
                        sigma_px: 16.0,
                    },
                    GlowLayer {
                        amplitude: 0.1444,
                        sigma_px: 48.0,
                    },
                ],
                edge_blend_px: 0.0,
            }),
            pulse: None,
            wavy: Some(WavySpec {
                amplitude_px: 9.5,
                wavelength_px: 55.0,
                speed: 2.0,
                mode: WavyMode::Edge,
                slide_speed: 40.0,
                strands: Some(StrandSpec {
                    count: 4,
                    spread_px: 4.0,
                    jitter: 1.0,
                    thickness_px: 2.0,
                    halo_amplitude: 1.0,
                    halo_sigma_px: 0.5,
                    glow_intensity: 0.5,
                    flicker_speed: 1.8,
                }),
            }),
            show_bar: false,
        }),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let sparks = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(visible_barrier()),
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
                layers: hot_layers(0.5, 1.0, 2.0),
            }),
            flash: Some(FlashSpec {
                radius_x_px: 40.0,
                radius_y_px: 40.0,
                color: ColorBinding::Constant([255, 255, 255]),
                decay_seconds: 0.15,
                mode: FlashMode::Instant,
                brightness: 1.0,
                layers: glow_layers(2.0, 5.0, 10.0),
            }),
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let ellipse_flash = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::Flash,
            particles: None,
            flash: Some(FlashSpec {
                radius_x_px: 70.0,
                radius_y_px: 40.0,
                color: ColorBinding::Constant([255, 255, 255]),
                decay_seconds: 0.2,
                mode: FlashMode::Instant,
                brightness: 1.0,
                layers: glow_layers(2.0, 5.0, 10.0),
            }),
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let grinding_particles = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(visible_barrier()),
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
                layers: glow_layers(0.5, 1.0, 2.0),
            }),
            flash: None,
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    let key_glow = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::Flash,
            particles: None,
            flash: Some(FlashSpec {
                radius_x_px: 20.0,
                radius_y_px: 5.0,
                color: ColorBinding::Constant([255, 220, 140]),
                decay_seconds: 0.1,
                mode: FlashMode::Sustained,
                brightness: 0.5,
                layers: glow_layers(2.0, 5.0, 10.0),
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
                layers: glow_layers(2.0, 4.0, 8.0),
                edge_blend_px: 0.0,
            }),
            show_bar: false,
            ..BarrierLayer::default()
        }),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([8, 10, 24]),
    };

    let showcase_blue_purple = Style {
        version: 1,
        notes: Timed::Static(NoteLayer {
            fill: Fill::VerticalGradient {
                top: ColorBinding::Constant([170, 235, 255]),
                bottom: ColorBinding::Constant([95, 55, 255]),
            },
            sheen: Some(Sheen {
                intensity: 0.42,
                width: 0.8,
                angle_degrees: 34.0,
            }),
            glow: Some(Glow {
                color: ColorBinding::Constant([190, 190, 255]),
                brightness: 0.3,
                layers: [
                    GlowLayer {
                        amplitude: 2.0,
                        sigma_px: 2.0,
                    },
                    GlowLayer {
                        amplitude: 1.5,
                        sigma_px: 4.0,
                    },
                    GlowLayer {
                        amplitude: 0.7,
                        sigma_px: 10.0,
                    },
                ],
                edge_blend_px: 6.0,
            }),
            roundedness: 1.65,
            fall_speed: 400.0,
            border: None,
            black_key_fill: BlackKeyFill::Custom(Fill::VerticalGradient {
                top: ColorBinding::Constant([170, 235, 255]),
                bottom: ColorBinding::Constant([95, 55, 255]),
            }),
        }),
        barrier: Timed::Static(BarrierLayer {
            color: ColorBinding::Constant([205, 245, 255]),
            thickness: 4.0,
            glow: Some(Glow {
                color: ColorBinding::Constant([135, 90, 255]),
                brightness: 1.5,
                layers: hot_layers(2.0, 4.0, 8.0),
                edge_blend_px: 0.0,
            }),
            pulse: Some(Pulse {
                decay_seconds: 0.34,
                brightness: 1.0,
            }),
            wavy: Some(WavySpec {
                amplitude_px: 15.0,
                wavelength_px: 150.0,
                speed: 1.0,
                mode: WavyMode::Edge,
                slide_speed: 0.0,
                strands: None,
            }),
            show_bar: false,
        }),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::ParticlesAndFlash,
            particles: Some(ParticleSpec {
                count: 40,
                lifetime_seconds: 1.0,
                size_px: 1.0,
                speed_px: 250.0,
                spread_degrees: 60.0,
                gravity_px: 220.0,
                color: ColorBinding::Constant([150, 210, 255]),
                additive: true,
                emission: project::EmissionMode::Continuous {
                    rate_per_second: 40.0,
                },
                brightness: 1.0,
                layers: [
                    GlowLayer {
                        amplitude: 3.0,
                        sigma_px: 0.5,
                    },
                    GlowLayer {
                        amplitude: 1.45,
                        sigma_px: 1.0,
                    },
                    GlowLayer {
                        amplitude: 0.58,
                        sigma_px: 2.0,
                    },
                ],
            }),
            flash: Some(FlashSpec {
                radius_x_px: 16.0,
                radius_y_px: 4.0,
                color: ColorBinding::Constant([205, 190, 255]),
                decay_seconds: 0.22,
                mode: FlashMode::Sustained,
                brightness: 1.0,
                layers: [
                    GlowLayer {
                        amplitude: 1.4,
                        sigma_px: 2.0,
                    },
                    GlowLayer {
                        amplitude: 0.7,
                        sigma_px: 5.0,
                    },
                    GlowLayer {
                        amplitude: 0.1,
                        sigma_px: 10.0,
                    },
                ],
            }),
        }),
        background: ColorBinding::Constant([4, 2, 14]),
    };

    // Phase P: `Fill::CanvasGradient` — color depends on the note's current position on the
    // canvas (deep blue near the top of the frame, warm gold approaching the barrier) rather than
    // each note's own local top/bottom, so every note reads the same color at a given height
    // regardless of pitch, and shifts color as it falls. `black_key_fill` is an independently
    // resolved `CanvasGradient` too (dimmer endpoints), just like other fills' `Custom` overrides.
    let canvas_gradient = Style {
        version: 1,
        notes: Timed::Static(NoteLayer {
            fill: Fill::CanvasGradient {
                top: ColorBinding::Constant([80, 120, 255]),
                bottom: ColorBinding::Constant([255, 200, 90]),
            },
            sheen: Some(Sheen {
                intensity: 0.35,
                width: 0.6,
                angle_degrees: 45.0,
            }),
            glow: None,
            roundedness: 1.0,
            fall_speed: 400.0,
            border: None,
            black_key_fill: BlackKeyFill::Custom(Fill::CanvasGradient {
                top: ColorBinding::Constant([40, 60, 130]),
                bottom: ColorBinding::Constant([160, 120, 50]),
            }),
        }),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    print_style("gradient-glow", &gradient_glow);
    print_style("canvas-gradient", &canvas_gradient);
    print_style("barrier-pulse", &barrier_pulse);
    print_style("barrier-wavy", &barrier_wavy);
    print_style("barrier-wavy-volume", &barrier_wavy_volume);
    print_style("barrier-strands", &barrier_strands);
    print_style("sparks", &sparks);
    print_style("ellipse-flash", &ellipse_flash);
    print_style("grinding-particles", &grinding_particles);
    print_style("key-glow", &key_glow);
    print_style("dark-background", &dark_background);
    print_style("showcase_blue_purple", &showcase_blue_purple);
}
