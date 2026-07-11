//! Throwaway generator for `examples/styles/*.fmstyle.ron` — builds each sample as real `Style`
//! values and serializes them, so the shipped files are guaranteed to match whatever RON syntax
//! this version of `ron` actually produces (rather than hand-typing RON that could drift from it).
//! Run with `cargo run -p project --example dump_sample_styles` and copy stdout sections into the
//! corresponding files if the schema ever changes.

use project::{
    BarrierLayer, BlackKeyFill, ColorBinding, Fill, FlashColor, FlashMode, FlashSpec, Glow,
    GlowLayer, GodRaySpec, NoteLayer, ParticleColor, ParticleSpec, Pulse, Ramp, RingSpec,
    ScalarBinding, Sheen, StrandSpec, Style, Timed, TransitionKind, TransitionLayer, WavyMode,
    WavySpec,
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
                match_note_color: false,
            }),
            roundedness: 1.0,
            fall_speed: 400.0,
            border: None,
            black_key_fill: BlackKeyFill::Auto,
            alpha: ScalarBinding::default(),
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
                match_note_color: false,
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
                match_note_color: false,
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
                match_note_color: false,
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

    // The strand bundle ported from `explorations/barrier-fx-lab` — several thin,
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
    //   `speed`/`slide_speed`/`mode` directly (see `WavySpec::slide_speed`'s own doc comment for
    //   how it differs from `speed`).
    // - `strandCount`/`strandSpread`/`strandJitter`/`strandThickness`/`strandHaloAmp`/
    //   `strandHaloSigma`/`strandGlow`/`strandFlicker` -> `StrandSpec`'s identically-purposed
    //   fields, 1:1, no translation needed. Several of these (`spread_px`/`thickness_px`/
    //   `glow_intensity`, and the barrier's own `thickness`) have since been hand-tuned away from
    //   a literal preset translation — see this style's own values below, not the preset JSON, for
    //   the current look.
    // - `filamentIntensity`/`filamentSpeed`/`filamentScale` (sliding filament) and
    //   `wispDensity`/`wispHeight`/`wispFlicker`/`wispSway`/`wispIntensity` (wisps) have no
    //   real-app equivalent — unported lab-only experiments.
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
                match_note_color: false,
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
                lifetime_seconds: ScalarBinding::Constant(0.4),
                size_px: ScalarBinding::Constant(4.0),
                speed_px: ScalarBinding::Constant(180.0),
                spread_degrees: ScalarBinding::Constant(60.0),
                gravity_px: ScalarBinding::Constant(300.0),
                color: ParticleColor::Fixed(ColorBinding::Constant([255, 240, 200])),
                additive: true,
                emission: project::EmissionMode::Burst,
                brightness: ScalarBinding::Constant(1.0),
                layers: hot_layers(0.5, 1.0, 2.0),
            }),
            flash: Some(FlashSpec {
                radius_x_px: ScalarBinding::Constant(40.0),
                radius_y_px: ScalarBinding::Constant(40.0),
                color: FlashColor::Solid(ColorBinding::Constant([255, 255, 255])),
                decay_seconds: ScalarBinding::Constant(0.15),
                mode: FlashMode::Instant,
                brightness: ScalarBinding::Constant(1.0),
                layers: glow_layers(2.0, 5.0, 10.0),
                flicker_speed: ScalarBinding::Constant(0.0),
                flicker_intensity: ScalarBinding::Constant(0.0),
                god_rays: None,
                ring: None,
                chromatic_aberration: 0.0,
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
                radius_x_px: ScalarBinding::Constant(70.0),
                radius_y_px: ScalarBinding::Constant(40.0),
                color: FlashColor::Solid(ColorBinding::Constant([255, 255, 255])),
                decay_seconds: ScalarBinding::Constant(0.2),
                mode: FlashMode::Instant,
                brightness: ScalarBinding::Constant(1.0),
                layers: glow_layers(2.0, 5.0, 10.0),
                flicker_speed: ScalarBinding::Constant(0.0),
                flicker_intensity: ScalarBinding::Constant(0.0),
                god_rays: None,
                ring: None,
                chromatic_aberration: 0.0,
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
                lifetime_seconds: ScalarBinding::Constant(0.45),
                size_px: ScalarBinding::Constant(6.0),
                speed_px: ScalarBinding::Constant(140.0),
                spread_degrees: ScalarBinding::Constant(100.0),
                gravity_px: ScalarBinding::Constant(250.0),
                color: ParticleColor::Fixed(ColorBinding::Constant([255, 210, 90])),
                additive: true,
                emission: project::EmissionMode::Continuous {
                    rate_per_second: 30.0,
                },
                brightness: ScalarBinding::Constant(1.0),
                layers: glow_layers(0.5, 1.0, 2.0),
            }),
            flash: None,
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // `flicker_speed`/`flicker_intensity` give the sustained hold a gentle candle-like waver
    // instead of a perfectly steady glow — subtle values here (a slow mutation rate, a shallow dim)
    // since a strong flicker on every held key would read as broken rather than atmospheric.
    let key_glow = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::Flash,
            particles: None,
            flash: Some(FlashSpec {
                radius_x_px: ScalarBinding::Constant(20.0),
                radius_y_px: ScalarBinding::Constant(5.0),
                color: FlashColor::Solid(ColorBinding::Constant([255, 220, 140])),
                decay_seconds: ScalarBinding::Constant(0.1),
                mode: FlashMode::Sustained,
                brightness: ScalarBinding::Constant(0.5),
                layers: glow_layers(2.0, 5.0, 10.0),
                flicker_speed: ScalarBinding::Constant(1.5),
                flicker_intensity: ScalarBinding::Constant(0.25),
                god_rays: None,
                ring: None,
                chromatic_aberration: 0.0,
            }),
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // A `FlashMode::Sustained` white flash with a strong, fast flicker and a near-point core, so it
    // reads as a radiating point light rather than a flat-topped spotlight.
    let flickering_flash = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::Flash,
            particles: None,
            flash: Some(FlashSpec {
                radius_x_px: ScalarBinding::Constant(3.0),
                radius_y_px: ScalarBinding::Constant(3.0),
                color: FlashColor::Solid(ColorBinding::Constant([255, 255, 255])),
                decay_seconds: ScalarBinding::Constant(0.15),
                mode: FlashMode::Sustained,
                brightness: ScalarBinding::Constant(1.0),
                layers: [
                    GlowLayer {
                        amplitude: 2.0,
                        sigma_px: 5.0,
                    },
                    GlowLayer {
                        amplitude: 2.0,
                        sigma_px: 8.0,
                    },
                    GlowLayer {
                        amplitude: 1.3,
                        sigma_px: 10.0,
                    },
                ],
                flicker_speed: ScalarBinding::Constant(10.0),
                // `flash_flicker`'s output is folded into alpha as `1.0 - intensity + intensity *
                // flick` (`rebuild_instances`), already clamped to `[0, 1]` at spawn
                // (`spawn_flash`) — `1.0` is the deepest a flicker can dim, so this is the max
                // meaningful value.
                flicker_intensity: ScalarBinding::Constant(0.5),
                god_rays: None,
                ring: None,
                chromatic_aberration: 0.0,
            }),
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // `GodRaySpec`/`RingSpec`/`FlashSpec::chromatic_aberration` (Phase V) — a straight translation
    // of `explorations/barrier-fx-lab`'s "Flash: photoreal sunburst" preset (its own "Export
    // settings" JSON output), the dialed-in target look for a "photograph of the sun from Earth"
    // flash: a tight near-point core, 24 wide volumetric rays fixed in place (no pulse/rotation —
    // `pulse_speed`/`pulse_amount`/`rotation_speed_deg_per_sec: 0.0`) but flickering hard and fast
    // per-beam (`flicker_speed: 4.16`, `flicker_intensity: 1.0`) so individual rays gutter and
    // reappear, a faint diffraction ring, and a small chromatic-aberration fringe at the outer
    // edge of the light. `flashYOffset: 200` in the lab preset has no equivalent here (a flash
    // always spawns at the triggering note's barrier position — same "lab-scene-only field" caveat
    // `barrier_strands`'s own comment calls out for `barrierYFrac`/`keyWidth`/`vignette`/
    // `exposure`); every other field below is a direct 1:1 field-name translation.
    let photoreal_sunburst = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::Flash,
            particles: None,
            flash: Some(FlashSpec {
                radius_x_px: ScalarBinding::Constant(6.0),
                radius_y_px: ScalarBinding::Constant(6.0),
                color: FlashColor::Solid(ColorBinding::Constant([255, 246, 224])),
                decay_seconds: ScalarBinding::Constant(0.2),
                mode: FlashMode::Sustained,
                brightness: ScalarBinding::Constant(0.4),
                layers: [
                    GlowLayer {
                        amplitude: 1.2,
                        sigma_px: 25.0,
                    },
                    GlowLayer {
                        amplitude: 0.9,
                        sigma_px: 50.0,
                    },
                    GlowLayer {
                        amplitude: 0.6,
                        sigma_px: 50.0,
                    },
                ],
                flicker_speed: ScalarBinding::Constant(0.0),
                flicker_intensity: ScalarBinding::Constant(0.0),
                god_rays: Some(GodRaySpec {
                    count: 32,
                    length_px: 72.0,
                    length_jitter: 0.0,
                    softness: 1.5,
                    rotation_offset_deg: 0.0,
                    rotation_speed_deg_per_sec: 30.0,
                    pulse_speed: 0.0,
                    pulse_amount: 0.0,
                    streakiness: 1.0,
                    flicker_speed: 4.0,
                    flicker_intensity: 1.0,
                    intensity: 0.50,
                }),
                ring: Some(RingSpec {
                    radius_px: 67.0,
                    width_px: 24.0,
                    intensity: 0.1,
                }),
                chromatic_aberration: 0.07,
            }),
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // `background` is the one field being demonstrated here — everything else stays at (or close
    // to) its default so the dark-navy canvas color, not any other effect, is what reads as the
    // point of this sample. `show_bar: false` on a modest glow keeps the barrier from reading as a
    // plain white line against the new background, without adding an unrelated look.
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
                match_note_color: false,
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
                match_note_color: false,
            }),
            roundedness: 1.65,
            fall_speed: 400.0,
            border: None,
            black_key_fill: BlackKeyFill::Custom(Fill::VerticalGradient {
                top: ColorBinding::Constant([170, 235, 255]),
                bottom: ColorBinding::Constant([95, 55, 255]),
            }),
            alpha: ScalarBinding::default(),
        }),
        barrier: Timed::Static(BarrierLayer {
            color: ColorBinding::Constant([205, 245, 255]),
            thickness: 4.0,
            glow: Some(Glow {
                color: ColorBinding::Constant([135, 90, 255]),
                brightness: 1.5,
                layers: hot_layers(2.0, 4.0, 8.0),
                edge_blend_px: 0.0,
                match_note_color: false,
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
                lifetime_seconds: ScalarBinding::Constant(1.0),
                size_px: ScalarBinding::Constant(1.0),
                speed_px: ScalarBinding::Constant(250.0),
                spread_degrees: ScalarBinding::Constant(60.0),
                gravity_px: ScalarBinding::Constant(220.0),
                color: ParticleColor::Fixed(ColorBinding::Constant([150, 210, 255])),
                additive: true,
                emission: project::EmissionMode::Continuous {
                    rate_per_second: 40.0,
                },
                brightness: ScalarBinding::Constant(1.0),
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
                radius_x_px: ScalarBinding::Constant(16.0),
                radius_y_px: ScalarBinding::Constant(4.0),
                color: FlashColor::Solid(ColorBinding::Constant([205, 190, 255])),
                decay_seconds: ScalarBinding::Constant(0.22),
                mode: FlashMode::Sustained,
                brightness: ScalarBinding::Constant(1.0),
                flicker_speed: ScalarBinding::Constant(0.0),
                flicker_intensity: ScalarBinding::Constant(0.0),
                god_rays: None,
                ring: None,
                chromatic_aberration: 0.0,
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

    // `Fill::CanvasGradient` — color depends on the note's current position on the
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
            alpha: ScalarBinding::default(),
        }),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // `ParticleColor::YGradient` — unlike `Fixed`/`MatchNote` (baked once at spawn), each
    // particle's color is recomputed every frame from its own *current* canvas Y position, blended
    // across `[top_fraction, bottom_fraction]` (a span of canvas height, not tied to the barrier).
    // A wide `spread_degrees` plus real `gravity_px` sends particles up past the barrier and back
    // down, so they visibly sweep from `bottom`'s red back toward `top`'s blue and down again as
    // they rise and fall, rather than holding one fixed color for their whole lifetime. `0.55`/
    // `0.85` brackets roughly where these particles actually travel (a bit above the default
    // barrier position at spawn, down to a bit below it) — the field's default `0.0`/`0.8` span
    // (top of frame to the barrier) would work here too, but most of these particles' motion would
    // land in a narrow sliver near the `bottom` end of that much wider span, so the color would
    // barely change.
    let ygradient_particles = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::Particles,
            particles: Some(ParticleSpec {
                count: 20,
                lifetime_seconds: ScalarBinding::Constant(1.1),
                size_px: ScalarBinding::Constant(1.0),
                speed_px: ScalarBinding::Constant(260.0),
                spread_degrees: ScalarBinding::Constant(90.0),
                gravity_px: ScalarBinding::Constant(150.0),
                color: ParticleColor::YGradient {
                    top: ColorBinding::Constant([60, 90, 255]),
                    bottom: ColorBinding::Constant([255, 60, 60]),
                    top_fraction: 0.55,
                    bottom_fraction: 0.66,
                },
                additive: true,
                emission: project::EmissionMode::Continuous {
                    rate_per_second: 50.0,
                },
                brightness: ScalarBinding::Constant(1.0),
                layers: hot_layers(0.5, 1.0, 2.0),
            }),
            flash: None,
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // Demonstrates the "match note color" family in one place: the note glow's corona/rim samples
    // the note's own gradient+sheen at whichever point it's closest to (rather than one fixed
    // `Glow::color`); the particle stream and the flash both derive their color from whichever
    // point of the note is currently at the barrier (see `project::ParticleColor::MatchNote`/
    // `project::FlashColor::MatchNote`) instead of a separately-authored fixed color. Particles use
    // `EmissionMode::Continuous` and the flash uses `FlashMode::Sustained` (rather than a one-shot
    // burst/`Instant` pulse) so the color-sliding part of `MatchNote` is actually visible: a held
    // note feeds a steady stream of particles, and keeps its flash lit, whose color keeps sliding
    // from the note's leading-edge color toward its trailing-edge color for as long as the note
    // stays held, instead of a one-shot cue frozen at the note's arrival color.
    let match_note_color = Style {
        version: 1,
        notes: Timed::Static(NoteLayer {
            fill: Fill::VerticalGradient {
                top: ColorBinding::Constant([255, 140, 60]),
                bottom: ColorBinding::Constant([60, 90, 255]),
            },
            sheen: Some(Sheen {
                intensity: 0.2,
                width: 0.6,
                angle_degrees: 40.0,
            }),
            glow: Some(Glow {
                color: ColorBinding::default(),
                brightness: 1.0,
                layers: glow_layers(2.0, 4.0, 10.0),
                edge_blend_px: 4.0,
                match_note_color: true,
            }),
            roundedness: 1.0,
            fall_speed: 400.0,
            border: None,
            black_key_fill: BlackKeyFill::Auto,
            alpha: ScalarBinding::default(),
        }),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::ParticlesAndFlash,
            particles: Some(ParticleSpec {
                count: 0,
                lifetime_seconds: ScalarBinding::Constant(1.0),
                size_px: ScalarBinding::Constant(1.0),
                speed_px: ScalarBinding::Constant(200.0),
                spread_degrees: ScalarBinding::Constant(70.0),
                gravity_px: ScalarBinding::Constant(200.0),
                color: ParticleColor::MatchNote,
                additive: true,
                emission: project::EmissionMode::Continuous {
                    rate_per_second: 25.0,
                },
                brightness: ScalarBinding::Constant(4.0),
                layers: hot_layers(0.5, 1.0, 2.0),
            }),
            flash: Some(FlashSpec {
                radius_x_px: ScalarBinding::Constant(20.0),
                radius_y_px: ScalarBinding::Constant(10.0),
                color: FlashColor::MatchNote,
                decay_seconds: ScalarBinding::Constant(0.18),
                mode: FlashMode::Sustained,
                brightness: ScalarBinding::Constant(2.0),
                layers: glow_layers(2.0, 5.0, 10.0),
                flicker_speed: ScalarBinding::Constant(0.0),
                flicker_intensity: ScalarBinding::Constant(0.0),
                god_rays: None,
                ring: None,
                chromatic_aberration: 0.0,
            }),
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // Demonstrates `ColorBinding::ByVelocity` actually resolving per note (rather than falling
    // back to `ramp.high` for every note): quiet notes render a cool, muted blue, loud notes a
    // hot orange-red, interpolated by each note's own MIDI velocity (0-127) via
    // `ColorBinding::resolve_for_note`.
    let velocity_colored_notes = Style {
        version: 1,
        notes: Timed::Static(NoteLayer {
            fill: Fill::Solid(ColorBinding::ByVelocity(Ramp {
                low: [40, 60, 120],
                high: [255, 90, 60],
            })),
            ..NoteLayer::default()
        }),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // Demonstrates `NoteLayer::alpha` (`ScalarBinding`) actually resolving per note: a soft
    // keypress (low velocity) renders as a mostly see-through note, a hard keypress (high
    // velocity) as fully opaque — the note core pipeline is already alpha-blended, so this needs
    // no renderer changes beyond baking the resolved value into `NoteInstance::alpha`.
    let note_alpha = Style {
        version: 1,
        notes: Timed::Static(NoteLayer {
            fill: Fill::Solid(ColorBinding::Constant([90, 200, 255])),
            glow: Some(Glow {
                color: ColorBinding::Constant([90, 200, 255]),
                brightness: 0.8,
                layers: glow_layers(2.0, 4.0, 10.0),
                edge_blend_px: 6.0,
                match_note_color: false,
            }),
            alpha: ScalarBinding::Constant(0.4),
            ..NoteLayer::default()
        }),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer::default()),
        // A warm, clearly-not-black background (rather than `dark_background`'s subtle navy)
        // deliberately contrasting the note's cool light-blue fill, so a transparent note visibly
        // shifts toward it instead of blending into another near-black.
        background: ColorBinding::Constant([150, 70, 50]),
    };

    // Demonstrates `ColorBinding::ByPitchClass` actually resolving per note: each of the 12 pitch
    // classes (C, C#, D, ... B — `pitch % 12`, independent of octave) gets its own fixed color, a
    // classic chromatic-circle rainbow rather than one color (`colors[0]`) for every note.
    let pitch_rainbow = Style {
        version: 1,
        notes: Timed::Static(NoteLayer {
            fill: Fill::Solid(ColorBinding::ByPitchClass([
                [255, 0, 0],   // C
                [255, 90, 0],  // C#
                [255, 180, 0], // D
                [220, 255, 0], // D#
                [130, 255, 0], // E
                [0, 255, 60],  // F
                [0, 255, 180], // F#
                [0, 200, 255], // G
                [0, 90, 255],  // G#
                [80, 0, 255],  // A
                [180, 0, 255], // A#
                [255, 0, 180], // B
            ])),
            ..NoteLayer::default()
        }),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // Demonstrates `ColorBinding::ByPitch` — unlike `ByPitchClass` just above (which repeats the
    // same 12 colors every octave), this scales continuously from the lowest key on the keyboard
    // to the highest: a deep blue at the very bottom of the 88-key range up through a warm orange
    // at the very top, so a bass note and the same pitch class an octave up read as visibly
    // different, and where a note sits on the whole keyboard (not just which of the 12 pitch
    // classes it is) is what the color communicates.
    let pitch_gradient = Style {
        version: 1,
        notes: Timed::Static(NoteLayer {
            fill: Fill::Solid(ColorBinding::ByPitch(Ramp {
                low: [40, 60, 220],
                high: [255, 160, 40],
            })),
            ..NoteLayer::default()
        }),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // Demonstrates `ColorBinding::ByTrack` actually resolving per note: each MIDI track index gets
    // its own fixed color (wrapping via `track_id % colors.len()` if a file has more tracks than
    // colors here) — useful for e.g. a two-track file where the right- and left-hand parts should
    // read as visually distinct rather than sharing one color.
    let track_colored_notes = Style {
        version: 1,
        notes: Timed::Static(NoteLayer {
            fill: Fill::Solid(ColorBinding::ByTrack(vec![
                [255, 200, 90],
                [90, 200, 255],
                [200, 90, 255],
            ])),
            ..NoteLayer::default()
        }),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer::default()),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    // Demonstrates `ColorBinding`/`ScalarBinding::ByVelocity` resolving per note outside of note
    // fill too — `ParticleColor::Fixed`/`FlashColor::Solid` (color) and `ParticleSpec::brightness`/
    // `FlashSpec::brightness` (brightness) both resolve against the *triggering* note's own
    // velocity (`render::effects`'s `resolve_particle_color`/`spawn_particles`/`spawn_flash`), so a
    // soft keypress sparks a dim, low-brightness ember-red burst and a hard keypress sparks a
    // bright, high-brightness white-hot one, instead of every arrival spawning an identical
    // fixed-color, fixed-brightness burst.
    let velocity_sparks = Style {
        version: 1,
        notes: Timed::Static(NoteLayer::default()),
        barrier: Timed::Static(visible_barrier()),
        transition: Timed::Static(TransitionLayer {
            kind: TransitionKind::ParticlesAndFlash,
            particles: Some(ParticleSpec {
                count: 24,
                lifetime_seconds: ScalarBinding::Constant(0.4),
                size_px: ScalarBinding::ByVelocity {
                    low: 2.0,
                    high: 5.0,
                },
                speed_px: ScalarBinding::ByVelocity {
                    low: 100.0,
                    high: 240.0,
                },
                spread_degrees: ScalarBinding::Constant(60.0),
                gravity_px: ScalarBinding::Constant(300.0),
                color: ParticleColor::Fixed(ColorBinding::ByVelocity(Ramp {
                    low: [120, 20, 20],
                    high: [255, 250, 220],
                })),
                additive: true,
                emission: project::EmissionMode::Burst,
                brightness: ScalarBinding::ByVelocity {
                    low: 0.3,
                    high: 1.6,
                },
                layers: hot_layers(0.5, 1.0, 2.0),
            }),
            flash: Some(FlashSpec {
                radius_x_px: ScalarBinding::ByVelocity {
                    low: 20.0,
                    high: 40.0,
                },
                radius_y_px: ScalarBinding::ByVelocity {
                    low: 5.0,
                    high: 10.0,
                },
                color: FlashColor::Solid(ColorBinding::ByVelocity(Ramp {
                    low: [120, 20, 20],
                    high: [255, 250, 220],
                })),
                decay_seconds: ScalarBinding::Constant(0.15),
                mode: FlashMode::Instant,
                brightness: ScalarBinding::ByVelocity {
                    low: 0.3,
                    high: 1.6,
                },
                layers: glow_layers(2.0, 5.0, 10.0),
                flicker_speed: ScalarBinding::Constant(0.0),
                flicker_intensity: ScalarBinding::Constant(0.0),
                god_rays: None,
                ring: None,
                chromatic_aberration: 0.0,
            }),
        }),
        background: ColorBinding::Constant([0, 0, 0]),
    };

    print_style("gradient-glow", &gradient_glow);
    print_style("canvas-gradient", &canvas_gradient);
    print_style("match-note-color", &match_note_color);
    print_style("barrier-pulse", &barrier_pulse);
    print_style("barrier-wavy", &barrier_wavy);
    print_style("barrier-wavy-volume", &barrier_wavy_volume);
    print_style("barrier-strands", &barrier_strands);
    print_style("sparks", &sparks);
    print_style("ellipse-flash", &ellipse_flash);
    print_style("grinding-particles", &grinding_particles);
    print_style("ygradient-particles", &ygradient_particles);
    print_style("key-glow", &key_glow);
    print_style("flickering-flash", &flickering_flash);
    print_style("photoreal-sunburst", &photoreal_sunburst);
    print_style("dark-background", &dark_background);
    print_style("showcase_blue_purple", &showcase_blue_purple);
    print_style("velocity-colored-notes", &velocity_colored_notes);
    print_style("note-alpha", &note_alpha);
    print_style("pitch-rainbow", &pitch_rainbow);
    print_style("pitch-gradient", &pitch_gradient);
    print_style("track-colored-notes", &track_colored_notes);
    print_style("velocity-sparks", &velocity_sparks);
}
