//! Reusable egui widgets for live-editing a `project::Style` — one editor function per schema
//! shape (`ColorBinding`/`ScalarBinding` variant pickers, `Option<T>`-gated sub-specs, glow
//! layers, fill/black-key-fill, particle/flash specs). Called from `ui::draw_style_tab`; see
//! `docs/fmstyle-format.md` for the field-by-field contract these widgets edit.

use egui::Ui;
use project::{
    BarrierLayer, BlackKeyFill, ColorBinding, EmissionMode, Fill, FlashColor, FlashMode, FlashSpec,
    Glow, GlowLayer, GodRaySpec, NoteLayer, OctaveLineSpec, ParticleColor, ParticleSpec, Pulse,
    Ramp, RingSpec, ScalarBinding, Sheen, StrandSpec, Timed, TransitionKind, TransitionLayer,
    WavyMode, WavySpec,
};

use crate::ui::{darken_color, validated_slider};

const PITCH_CLASS_LABELS: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Shows a checkbox toggling `opt` between `None` and `Some(default())`, drawing `body` (indented)
/// only while `Some`. The enable/disable pattern used everywhere a layer field is optional
/// (`sheen`/`glow`/`pulse`/`wavy`/`strands`/`god_rays`/`ring`/`octave_lines`/etc.).
pub(crate) fn optional_section<T>(
    ui: &mut Ui,
    label: &str,
    opt: &mut Option<T>,
    default: impl FnOnce() -> T,
    body: impl FnOnce(&mut Ui, &mut T),
) {
    let mut enabled = opt.is_some();
    ui.checkbox(&mut enabled, label);
    if enabled && opt.is_none() {
        *opt = Some(default());
    } else if !enabled && opt.is_some() {
        *opt = None;
    }
    if let Some(value) = opt {
        ui.indent(label, |ui| body(ui, value));
    }
}

/// If `timed` is `Keyed`, shows a notice and an "Edit as static" button that flattens it (see
/// `Timed::flatten_to_static`) and returns `None` for this call — the caller should skip drawing
/// editing controls for that layer this frame. Returns `Some(&mut T)` once (already, or just now)
/// `Static`. v1 only ever resolves `Timed<T>` at `t = 0.0`, so the Style tab never edits `Keyed`
/// timelines directly — see `docs/fmstyle-format.md`'s `Timed<T>` section.
pub(crate) fn timed_static_mut<'a, T: Clone>(
    ui: &mut Ui,
    timed: &'a mut Timed<T>,
) -> Option<&'a mut T> {
    if timed.is_keyed() {
        ui.colored_label(
            egui::Color32::from_rgb(240, 200, 80),
            "Time-keyed layer — editing here flattens it to one static look.",
        );
        if ui.button("Edit as static").clicked() {
            timed.flatten_to_static();
        }
        None
    } else {
        Some(timed.static_mut())
    }
}

fn variant_combo(
    ui: &mut Ui,
    id: &str,
    current: &str,
    variants: &'static [&'static str],
) -> Option<&'static str> {
    let mut picked = None;
    egui::ComboBox::from_id_salt(id)
        .selected_text(current.to_string())
        .show_ui(ui, |ui| {
            for variant in variants {
                if ui.selectable_label(current == *variant, *variant).clicked() {
                    picked = Some(*variant);
                }
            }
        });
    picked
}

const COLOR_BINDING_VARIANTS: [&str; 5] = [
    "Constant",
    "By velocity",
    "By pitch class",
    "By pitch",
    "By track",
];

pub(crate) fn edit_color_binding(ui: &mut Ui, id: &str, label: &str, binding: &mut ColorBinding) {
    let current = match binding {
        ColorBinding::Constant(_) => "Constant",
        ColorBinding::ByVelocity(_) => "By velocity",
        ColorBinding::ByPitchClass(_) => "By pitch class",
        ColorBinding::ByPitch(_) => "By pitch",
        ColorBinding::ByTrack(_) => "By track",
    };
    ui.horizontal(|ui| {
        ui.label(label);
        if let Some(picked) = variant_combo(ui, id, current, &COLOR_BINDING_VARIANTS) {
            let c = binding.resolve_constant();
            *binding = match picked {
                "Constant" => ColorBinding::Constant(c),
                "By velocity" => ColorBinding::ByVelocity(Ramp { low: c, high: c }),
                "By pitch class" => ColorBinding::ByPitchClass([c; 12]),
                "By pitch" => ColorBinding::ByPitch(Ramp { low: c, high: c }),
                "By track" => ColorBinding::ByTrack(vec![c]),
                _ => unreachable!(),
            };
        }
    });
    match binding {
        ColorBinding::Constant(color) => {
            ui.color_edit_button_srgb(color);
        }
        ColorBinding::ByVelocity(ramp) | ColorBinding::ByPitch(ramp) => {
            ui.horizontal(|ui| {
                ui.label("Low:");
                ui.color_edit_button_srgb(&mut ramp.low);
                ui.label("High:");
                ui.color_edit_button_srgb(&mut ramp.high);
            });
        }
        ColorBinding::ByPitchClass(colors) => edit_pitch_class_colors(ui, colors),
        ColorBinding::ByTrack(colors) => edit_track_colors(ui, id, colors),
    }
}

fn edit_pitch_class_colors(ui: &mut Ui, colors: &mut [[u8; 3]; 12]) {
    ui.horizontal_wrapped(|ui| {
        for (i, color) in colors.iter_mut().enumerate() {
            ui.vertical(|ui| {
                ui.label(PITCH_CLASS_LABELS[i]);
                ui.color_edit_button_srgb(color);
            });
        }
    });
}

fn edit_track_colors(ui: &mut Ui, id: &str, colors: &mut Vec<[u8; 3]>) {
    ui.push_id(id, |ui| {
        for (i, color) in colors.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("Track {i}:"));
                ui.color_edit_button_srgb(color);
            });
        }
        ui.horizontal(|ui| {
            if ui.button("+ Add").clicked() {
                colors.push([255, 255, 255]);
            }
            if colors.len() > 1 && ui.button("- Remove last").clicked() {
                colors.pop();
            }
        });
    });
}

const SCALAR_BINDING_VARIANTS: [&str; 5] = [
    "Constant",
    "By velocity",
    "By pitch class",
    "By pitch",
    "By track",
];

pub(crate) fn edit_scalar_binding(
    ui: &mut Ui,
    id: &str,
    label: &str,
    binding: &mut ScalarBinding,
    range: std::ops::RangeInclusive<f32>,
) {
    let current = match binding {
        ScalarBinding::Constant(_) => "Constant",
        ScalarBinding::ByVelocity { .. } => "By velocity",
        ScalarBinding::ByPitchClass(_) => "By pitch class",
        ScalarBinding::ByPitch { .. } => "By pitch",
        ScalarBinding::ByTrack(_) => "By track",
    };
    ui.horizontal(|ui| {
        ui.label(label);
        if let Some(picked) = variant_combo(ui, id, current, &SCALAR_BINDING_VARIANTS) {
            let v = binding.resolve_constant();
            *binding = match picked {
                "Constant" => ScalarBinding::Constant(v),
                "By velocity" => ScalarBinding::ByVelocity { low: v, high: v },
                "By pitch class" => ScalarBinding::ByPitchClass([v; 12]),
                "By pitch" => ScalarBinding::ByPitch { low: v, high: v },
                "By track" => ScalarBinding::ByTrack(vec![v]),
                _ => unreachable!(),
            };
        }
    });
    match binding {
        ScalarBinding::Constant(value) => {
            validated_slider(ui, value, range, None);
        }
        ScalarBinding::ByVelocity { low, high } | ScalarBinding::ByPitch { low, high } => {
            ui.horizontal(|ui| {
                ui.label("Low:");
                validated_slider(ui, low, range.clone(), None);
            });
            ui.horizontal(|ui| {
                ui.label("High:");
                validated_slider(ui, high, range, None);
            });
        }
        ScalarBinding::ByPitchClass(values) => edit_pitch_class_scalars(ui, values, range),
        ScalarBinding::ByTrack(values) => edit_track_scalars(ui, id, values, range),
    }
}

fn edit_pitch_class_scalars(
    ui: &mut Ui,
    values: &mut [f32; 12],
    range: std::ops::RangeInclusive<f32>,
) {
    ui.horizontal_wrapped(|ui| {
        for (i, value) in values.iter_mut().enumerate() {
            ui.vertical(|ui| {
                ui.label(PITCH_CLASS_LABELS[i]);
                ui.add(egui::DragValue::new(value).range(range.clone()).speed(0.01));
            });
        }
    });
}

fn edit_track_scalars(
    ui: &mut Ui,
    id: &str,
    values: &mut Vec<f32>,
    range: std::ops::RangeInclusive<f32>,
) {
    ui.push_id(id, |ui| {
        for (i, value) in values.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("Track {i}:"));
                ui.add(egui::DragValue::new(value).range(range.clone()).speed(0.01));
            });
        }
        ui.horizontal(|ui| {
            if ui.button("+ Add").clicked() {
                values.push(*range.start());
            }
            if values.len() > 1 && ui.button("- Remove last").clicked() {
                values.pop();
            }
        });
    });
}

/// A single representative color for whichever `ColorBinding` currently drives `fill`'s "primary"
/// endpoint (`Solid`'s own color, or a gradient's `top`) — used to seed a sensible starting color
/// when switching `Fill`/`BlackKeyFill` variants instead of jumping to an arbitrary default.
fn fill_primary_color(fill: &Fill) -> [u8; 3] {
    match fill {
        Fill::Solid(c) => c.resolve_constant(),
        Fill::VerticalGradient { top, .. } | Fill::CanvasGradient { top, .. } => {
            top.resolve_constant()
        }
    }
}

const FILL_VARIANTS: [&str; 3] = [
    "Solid",
    "Vertical gradient (per-note)",
    "Canvas gradient (per-position)",
];

pub(crate) fn edit_fill(ui: &mut Ui, id: &str, fill: &mut Fill) {
    let current = match fill {
        Fill::Solid(_) => "Solid",
        Fill::VerticalGradient { .. } => "Vertical gradient (per-note)",
        Fill::CanvasGradient { .. } => "Canvas gradient (per-position)",
    };
    ui.horizontal(|ui| {
        ui.label("Fill:");
        if let Some(picked) = variant_combo(ui, id, current, &FILL_VARIANTS) {
            let top = fill_primary_color(fill);
            let bottom = darken_color(top, 0.6);
            *fill = match picked {
                "Solid" => Fill::Solid(ColorBinding::Constant(top)),
                "Vertical gradient (per-note)" => Fill::VerticalGradient {
                    top: ColorBinding::Constant(top),
                    bottom: ColorBinding::Constant(bottom),
                },
                "Canvas gradient (per-position)" => Fill::CanvasGradient {
                    top: ColorBinding::Constant(top),
                    bottom: ColorBinding::Constant(bottom),
                },
                _ => unreachable!(),
            };
        }
    });
    match fill {
        Fill::Solid(color) => edit_color_binding(ui, &format!("{id}_solid"), "Color", color),
        Fill::VerticalGradient { top, bottom } | Fill::CanvasGradient { top, bottom } => {
            edit_color_binding(ui, &format!("{id}_top"), "Top", top);
            edit_color_binding(ui, &format!("{id}_bottom"), "Bottom", bottom);
        }
    }
}

const BLACK_KEY_FILL_VARIANTS: [&str; 3] = ["Auto", "Same", "Custom"];

pub(crate) fn edit_black_key_fill(
    ui: &mut Ui,
    id: &str,
    black_key_fill: &mut BlackKeyFill,
    natural_fill: &Fill,
) {
    let current = match black_key_fill {
        BlackKeyFill::Auto => "Auto",
        BlackKeyFill::Same => "Same",
        BlackKeyFill::Custom(_) => "Custom",
    };
    ui.horizontal(|ui| {
        ui.label("Black keys:");
        if let Some(picked) = variant_combo(ui, id, current, &BLACK_KEY_FILL_VARIANTS) {
            *black_key_fill = match picked {
                "Auto" => BlackKeyFill::Auto,
                "Same" => BlackKeyFill::Same,
                "Custom" => BlackKeyFill::Custom(Fill::Solid(ColorBinding::Constant(
                    darken_color(fill_primary_color(natural_fill), 0.6),
                ))),
                _ => unreachable!(),
            };
        }
    });
    if let BlackKeyFill::Custom(fill) = black_key_fill {
        edit_fill(ui, &format!("{id}_custom"), fill);
    }
}

pub(crate) fn edit_sheen(ui: &mut Ui, sheen: &mut Sheen) {
    ui.horizontal(|ui| {
        ui.label("Intensity:");
        validated_slider(ui, &mut sheen.intensity, 0.0..=2.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Width:");
        validated_slider(ui, &mut sheen.width, 0.0..=1.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Angle (deg):");
        validated_slider(ui, &mut sheen.angle_degrees, -180.0..=180.0, None);
    });
}

pub(crate) fn edit_glow_layers(ui: &mut Ui, layers: &mut [GlowLayer; 3]) {
    for (i, layer) in layers.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("Layer {}:", i + 1));
            ui.label("Amplitude");
            ui.add(
                egui::DragValue::new(&mut layer.amplitude)
                    .speed(0.02)
                    .range(0.0..=20.0),
            );
            ui.label("Sigma px");
            ui.add(
                egui::DragValue::new(&mut layer.sigma_px)
                    .speed(0.2)
                    .range(0.0..=300.0),
            );
        });
    }
}

/// `barrier_context: true` shows a hover-note next to fields that are documented no-ops on
/// `BarrierLayer::glow` (`edge_blend_px`/`match_note_color` — see `docs/fmstyle-format.md`'s
/// "known schema-only/no-op fields" section). The same `Glow` struct backs both `NoteLayer::glow`
/// and `BarrierLayer::glow`, so this widget is shared rather than duplicated.
pub(crate) fn edit_glow(ui: &mut Ui, id: &str, glow: &mut Glow, barrier_context: bool) {
    edit_color_binding(ui, &format!("{id}_color"), "Color", &mut glow.color);
    ui.horizontal(|ui| {
        ui.label("Brightness:");
        validated_slider(ui, &mut glow.brightness, 0.0..=4.0, None);
    });
    ui.label("Corona layers:");
    edit_glow_layers(ui, &mut glow.layers);
    ui.horizontal(|ui| {
        ui.label("Edge blend (px):");
        validated_slider(ui, &mut glow.edge_blend_px, 0.0..=20.0, None);
        if barrier_context {
            ui.weak("(notes-only, no effect here)");
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut glow.match_note_color, "Match note color");
        if barrier_context {
            ui.weak("(notes-only, no effect here)");
        }
    });
}

pub(crate) fn edit_note_layer(ui: &mut Ui, layer: &mut NoteLayer) {
    edit_fill(ui, "note_fill", &mut layer.fill);
    edit_black_key_fill(
        ui,
        "note_black_key_fill",
        &mut layer.black_key_fill,
        &layer.fill,
    );
    ui.separator();
    optional_section(
        ui,
        "Sheen",
        &mut layer.sheen,
        || Sheen {
            intensity: 0.5,
            width: 0.8,
            angle_degrees: 45.0,
        },
        edit_sheen,
    );
    optional_section(ui, "Glow", &mut layer.glow, Glow::default, |ui, glow| {
        edit_glow(ui, "note_glow", glow, false)
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Roundedness:");
        validated_slider(ui, &mut layer.roundedness, 0.0..=3.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Fall speed:");
        validated_slider(ui, &mut layer.fall_speed, 50.0..=2000.0, Some(0));
    })
    .response
    .on_hover_text(
        "Also changes how long each note looks, since a note's on-screen length is its \
        duration times this speed.",
    );
    edit_scalar_binding(ui, "note_alpha", "Alpha", &mut layer.alpha, 0.0..=1.0);
}

fn default_wavy_spec() -> WavySpec {
    WavySpec {
        amplitude_px: 6.0,
        wavelength_px: 220.0,
        speed: 18.0,
        mode: WavyMode::Edge,
        slide_speed: 0.0,
        strands: None,
    }
}

const WAVY_MODE_VARIANTS: [&str; 3] = ["Top wave", "Edge", "Full wave"];

pub(crate) fn edit_wavy_spec(ui: &mut Ui, wavy: &mut WavySpec) {
    ui.horizontal(|ui| {
        ui.label("Amplitude (px):");
        validated_slider(ui, &mut wavy.amplitude_px, 0.0..=40.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Wavelength (px):");
        validated_slider(ui, &mut wavy.wavelength_px, 10.0..=400.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Speed:");
        validated_slider(ui, &mut wavy.speed, 0.0..=30.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Slide speed (px/s):");
        validated_slider(ui, &mut wavy.slide_speed, -100.0..=100.0, None);
    });
    let current = match wavy.mode {
        WavyMode::TopWave => "Top wave",
        WavyMode::Edge => "Edge",
        WavyMode::FullWave => "Full wave",
    };
    ui.horizontal(|ui| {
        ui.label("Mode:");
        if let Some(picked) = variant_combo(ui, "wavy_mode", current, &WAVY_MODE_VARIANTS) {
            wavy.mode = match picked {
                "Top wave" => WavyMode::TopWave,
                "Edge" => WavyMode::Edge,
                "Full wave" => WavyMode::FullWave,
                _ => unreachable!(),
            };
        }
    });
    optional_section(
        ui,
        "Strand bundle",
        &mut wavy.strands,
        StrandSpec::default,
        edit_strand_spec,
    );
}

pub(crate) fn edit_strand_spec(ui: &mut Ui, strands: &mut StrandSpec) {
    ui.weak("Only visible when the barrier has glow enabled and mode is Edge.");
    ui.horizontal(|ui| {
        ui.label("Count:");
        ui.add(egui::DragValue::new(&mut strands.count).range(1..=8));
    });
    ui.horizontal(|ui| {
        ui.label("Spread (px):");
        validated_slider(ui, &mut strands.spread_px, 0.0..=60.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Jitter:");
        validated_slider(ui, &mut strands.jitter, 0.0..=1.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Thickness (px):");
        validated_slider(ui, &mut strands.thickness_px, 0.2..=6.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Halo amplitude:");
        validated_slider(ui, &mut strands.halo_amplitude, 0.0..=5.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Halo sigma (px):");
        validated_slider(ui, &mut strands.halo_sigma_px, 0.0..=40.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Glow intensity:");
        validated_slider(ui, &mut strands.glow_intensity, 0.0..=5.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Flicker speed:");
        validated_slider(ui, &mut strands.flicker_speed, 0.0..=10.0, None);
    });
}

pub(crate) fn edit_barrier_layer(ui: &mut Ui, layer: &mut BarrierLayer) {
    edit_color_binding(ui, "barrier_color", "Color", &mut layer.color);
    ui.horizontal(|ui| {
        ui.label("Thickness:");
        validated_slider(ui, &mut layer.thickness, 1.0..=12.0, None);
    });
    ui.checkbox(&mut layer.show_bar, "Show solid bar");
    ui.separator();
    optional_section(ui, "Glow", &mut layer.glow, Glow::default, |ui, glow| {
        edit_glow(ui, "barrier_glow", glow, true)
    });
    optional_section(
        ui,
        "Pulse",
        &mut layer.pulse,
        || Pulse {
            decay_seconds: 0.35,
            brightness: 1.6,
        },
        |ui, pulse| {
            ui.horizontal(|ui| {
                ui.label("Decay (s):");
                validated_slider(ui, &mut pulse.decay_seconds, 0.02..=2.0, None);
            });
            ui.horizontal(|ui| {
                ui.label("Brightness:");
                validated_slider(ui, &mut pulse.brightness, 0.0..=4.0, None);
            });
        },
    );
    optional_section(
        ui,
        "Wavy edge",
        &mut layer.wavy,
        default_wavy_spec,
        edit_wavy_spec,
    );
}

fn default_particle_spec() -> ParticleSpec {
    ParticleSpec {
        count: 24,
        lifetime_seconds: ScalarBinding::Constant(0.4),
        size_px: ScalarBinding::Constant(4.0),
        speed_px: ScalarBinding::Constant(180.0),
        spread_degrees: ScalarBinding::Constant(60.0),
        gravity_px: ScalarBinding::Constant(300.0),
        color: ParticleColor::Fixed(ColorBinding::Constant([255, 240, 200])),
        additive: true,
        emission: EmissionMode::Burst,
        brightness: ScalarBinding::Constant(1.0),
        layers: [
            GlowLayer {
                amplitude: 3.0,
                sigma_px: 0.5,
            },
            GlowLayer {
                amplitude: 2.0,
                sigma_px: 1.0,
            },
            GlowLayer {
                amplitude: 0.85,
                sigma_px: 2.0,
            },
        ],
    }
}

const PARTICLE_COLOR_VARIANTS: [&str; 3] = ["Fixed", "Match note", "Y gradient"];

pub(crate) fn edit_particle_color(ui: &mut Ui, id: &str, color: &mut ParticleColor) {
    let current = match color {
        ParticleColor::Fixed(_) => "Fixed",
        ParticleColor::MatchNote => "Match note",
        ParticleColor::YGradient { .. } => "Y gradient",
    };
    ui.horizontal(|ui| {
        ui.label("Color mode:");
        if let Some(picked) = variant_combo(ui, id, current, &PARTICLE_COLOR_VARIANTS) {
            *color = match picked {
                "Fixed" => ParticleColor::Fixed(ColorBinding::default()),
                "Match note" => ParticleColor::MatchNote,
                "Y gradient" => ParticleColor::YGradient {
                    top: ColorBinding::default(),
                    bottom: ColorBinding::Constant([255, 200, 120]),
                    top_fraction: 0.0,
                    bottom_fraction: 0.8,
                },
                _ => unreachable!(),
            };
        }
    });
    match color {
        ParticleColor::Fixed(binding) => {
            edit_color_binding(ui, &format!("{id}_fixed"), "Color", binding)
        }
        ParticleColor::MatchNote => {}
        ParticleColor::YGradient {
            top,
            bottom,
            top_fraction,
            bottom_fraction,
        } => {
            edit_color_binding(ui, &format!("{id}_top"), "Top", top);
            edit_color_binding(ui, &format!("{id}_bottom"), "Bottom", bottom);
            ui.horizontal(|ui| {
                ui.label("Top fraction:");
                validated_slider(ui, top_fraction, 0.0..=1.0, None);
            });
            ui.horizontal(|ui| {
                ui.label("Bottom fraction:");
                validated_slider(ui, bottom_fraction, 0.0..=1.0, None);
            });
        }
    }
}

pub(crate) fn edit_particle_spec(ui: &mut Ui, spec: &mut ParticleSpec) {
    ui.horizontal(|ui| {
        ui.label("Count:");
        ui.add(egui::DragValue::new(&mut spec.count).range(0..=300));
    });
    edit_scalar_binding(
        ui,
        "particle_lifetime",
        "Lifetime (s)",
        &mut spec.lifetime_seconds,
        0.02..=5.0,
    );
    edit_scalar_binding(
        ui,
        "particle_size",
        "Size (px)",
        &mut spec.size_px,
        0.5..=40.0,
    );
    edit_scalar_binding(
        ui,
        "particle_speed",
        "Speed (px/s)",
        &mut spec.speed_px,
        0.0..=1000.0,
    );
    edit_scalar_binding(
        ui,
        "particle_spread",
        "Spread (deg)",
        &mut spec.spread_degrees,
        0.0..=360.0,
    );
    edit_scalar_binding(
        ui,
        "particle_gravity",
        "Gravity (px/s\u{b2})",
        &mut spec.gravity_px,
        0.0..=2000.0,
    );
    edit_particle_color(ui, "particle_color", &mut spec.color);
    ui.checkbox(&mut spec.additive, "Additive blending");
    let current = match spec.emission {
        EmissionMode::Burst => "Burst",
        EmissionMode::Continuous { .. } => "Continuous",
    };
    ui.horizontal(|ui| {
        ui.label("Emission:");
        if let Some(picked) =
            variant_combo(ui, "particle_emission", current, &["Burst", "Continuous"])
        {
            spec.emission = match picked {
                "Burst" => EmissionMode::Burst,
                "Continuous" => EmissionMode::Continuous {
                    rate_per_second: 60.0,
                },
                _ => unreachable!(),
            };
        }
    });
    if let EmissionMode::Continuous { rate_per_second } = &mut spec.emission {
        ui.horizontal(|ui| {
            ui.label("Rate (particles/s):");
            validated_slider(ui, rate_per_second, 1.0..=500.0, None);
        });
    }
    edit_scalar_binding(
        ui,
        "particle_brightness",
        "Brightness",
        &mut spec.brightness,
        0.0..=4.0,
    );
    if spec.additive {
        ui.label("Corona layers (additive only):");
        edit_glow_layers(ui, &mut spec.layers);
    }
}

fn default_flash_spec() -> FlashSpec {
    FlashSpec {
        radius_x_px: ScalarBinding::Constant(40.0),
        radius_y_px: ScalarBinding::Constant(40.0),
        color: FlashColor::Solid(ColorBinding::Constant([255, 255, 255])),
        decay_seconds: ScalarBinding::Constant(0.15),
        mode: FlashMode::Instant,
        brightness: ScalarBinding::Constant(1.0),
        layers: [
            GlowLayer {
                amplitude: 2.6,
                sigma_px: 2.0,
            },
            GlowLayer {
                amplitude: 1.1,
                sigma_px: 5.0,
            },
            GlowLayer {
                amplitude: 0.38,
                sigma_px: 10.0,
            },
        ],
        flicker_speed: ScalarBinding::Constant(0.0),
        flicker_intensity: ScalarBinding::Constant(0.0),
        god_rays: None,
        ring: None,
        chromatic_aberration: 0.0,
    }
}

const FLASH_COLOR_VARIANTS: [&str; 3] = ["Solid", "Horizontal gradient", "Match note"];

pub(crate) fn edit_flash_color(ui: &mut Ui, id: &str, color: &mut FlashColor) {
    let current = match color {
        FlashColor::Solid(_) => "Solid",
        FlashColor::HorizontalGradient(_) => "Horizontal gradient",
        FlashColor::MatchNote => "Match note",
    };
    ui.horizontal(|ui| {
        ui.label("Color mode:");
        if let Some(picked) = variant_combo(ui, id, current, &FLASH_COLOR_VARIANTS) {
            *color = match picked {
                "Solid" => FlashColor::Solid(ColorBinding::default()),
                "Horizontal gradient" => FlashColor::HorizontalGradient(vec![
                    ColorBinding::default(),
                    ColorBinding::Constant([255, 200, 120]),
                ]),
                "Match note" => FlashColor::MatchNote,
                _ => unreachable!(),
            };
        }
    });
    match color {
        FlashColor::Solid(binding) => {
            edit_color_binding(ui, &format!("{id}_solid"), "Color", binding)
        }
        FlashColor::HorizontalGradient(stops) => {
            for (i, stop) in stops.iter_mut().enumerate() {
                edit_color_binding(
                    ui,
                    &format!("{id}_stop_{i}"),
                    &format!("Stop {}", i + 1),
                    stop,
                );
            }
            ui.horizontal(|ui| {
                if ui.button("+ Add stop").clicked() {
                    let last = stops
                        .last()
                        .map(|c| c.resolve_constant())
                        .unwrap_or([255, 255, 255]);
                    stops.push(ColorBinding::Constant(last));
                }
                if stops.len() > 1 && ui.button("- Remove stop").clicked() {
                    stops.pop();
                }
            });
        }
        FlashColor::MatchNote => {}
    }
}

pub(crate) fn edit_god_ray_spec(ui: &mut Ui, spec: &mut GodRaySpec) {
    ui.horizontal(|ui| {
        ui.label("Count:");
        ui.add(egui::DragValue::new(&mut spec.count).range(1..=128));
    });
    ui.horizontal(|ui| {
        ui.label("Length (px):");
        validated_slider(ui, &mut spec.length_px, 0.0..=300.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Length jitter:");
        validated_slider(ui, &mut spec.length_jitter, 0.0..=1.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Softness:");
        validated_slider(ui, &mut spec.softness, 0.1..=8.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Rotation offset (deg):");
        validated_slider(ui, &mut spec.rotation_offset_deg, -180.0..=180.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Rotation speed (deg/s):");
        validated_slider(
            ui,
            &mut spec.rotation_speed_deg_per_sec,
            -180.0..=180.0,
            None,
        );
    });
    ui.horizontal(|ui| {
        ui.label("Pulse speed:");
        validated_slider(ui, &mut spec.pulse_speed, 0.0..=10.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Pulse amount:");
        validated_slider(ui, &mut spec.pulse_amount, 0.0..=1.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Streakiness:");
        validated_slider(ui, &mut spec.streakiness, 0.0..=1.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Flicker speed:");
        validated_slider(ui, &mut spec.flicker_speed, 0.0..=10.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Flicker intensity:");
        validated_slider(ui, &mut spec.flicker_intensity, 0.0..=1.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Intensity:");
        validated_slider(ui, &mut spec.intensity, 0.0..=3.0, None);
    });
}

pub(crate) fn edit_ring_spec(ui: &mut Ui, spec: &mut RingSpec) {
    ui.horizontal(|ui| {
        ui.label("Radius (px):");
        validated_slider(ui, &mut spec.radius_px, 0.0..=300.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Width (px):");
        validated_slider(ui, &mut spec.width_px, 0.5..=60.0, None);
    });
    ui.horizontal(|ui| {
        ui.label("Intensity:");
        validated_slider(ui, &mut spec.intensity, 0.0..=3.0, None);
    });
}

pub(crate) fn edit_flash_spec(ui: &mut Ui, spec: &mut FlashSpec) {
    edit_scalar_binding(
        ui,
        "flash_radius_x",
        "Radius X (px)",
        &mut spec.radius_x_px,
        1.0..=300.0,
    );
    edit_scalar_binding(
        ui,
        "flash_radius_y",
        "Radius Y (px)",
        &mut spec.radius_y_px,
        1.0..=300.0,
    );
    edit_flash_color(ui, "flash_color", &mut spec.color);
    edit_scalar_binding(
        ui,
        "flash_decay",
        "Decay (s)",
        &mut spec.decay_seconds,
        0.02..=3.0,
    );
    let current = match spec.mode {
        FlashMode::Instant => "Instant",
        FlashMode::Sustained => "Sustained",
    };
    ui.horizontal(|ui| {
        ui.label("Mode:");
        if let Some(picked) = variant_combo(ui, "flash_mode", current, &["Instant", "Sustained"]) {
            spec.mode = match picked {
                "Instant" => FlashMode::Instant,
                "Sustained" => FlashMode::Sustained,
                _ => unreachable!(),
            };
        }
    });
    edit_scalar_binding(
        ui,
        "flash_brightness",
        "Brightness",
        &mut spec.brightness,
        0.0..=4.0,
    );
    ui.label("Corona layers:");
    edit_glow_layers(ui, &mut spec.layers);
    edit_scalar_binding(
        ui,
        "flash_flicker_speed",
        "Flicker speed",
        &mut spec.flicker_speed,
        0.0..=10.0,
    );
    edit_scalar_binding(
        ui,
        "flash_flicker_intensity",
        "Flicker intensity",
        &mut spec.flicker_intensity,
        0.0..=1.0,
    );
    ui.separator();
    optional_section(
        ui,
        "God rays",
        &mut spec.god_rays,
        GodRaySpec::default,
        edit_god_ray_spec,
    );
    optional_section(
        ui,
        "Ring",
        &mut spec.ring,
        || RingSpec {
            radius_px: 60.0,
            width_px: 12.0,
            intensity: 1.0,
        },
        edit_ring_spec,
    );
    ui.horizontal(|ui| {
        ui.label("Chromatic aberration:");
        validated_slider(ui, &mut spec.chromatic_aberration, 0.0..=0.15, None);
    });
}

const TRANSITION_KIND_VARIANTS: [&str; 4] = ["None", "Particles", "Flash", "Particles + Flash"];

pub(crate) fn edit_transition_layer(ui: &mut Ui, layer: &mut TransitionLayer) {
    let current = match layer.kind {
        TransitionKind::None => "None",
        TransitionKind::Particles => "Particles",
        TransitionKind::Flash => "Flash",
        TransitionKind::ParticlesAndFlash => "Particles + Flash",
    };
    ui.horizontal(|ui| {
        ui.label("Kind:");
        if let Some(picked) =
            variant_combo(ui, "transition_kind", current, &TRANSITION_KIND_VARIANTS)
        {
            layer.kind = match picked {
                "None" => TransitionKind::None,
                "Particles" => TransitionKind::Particles,
                "Flash" => TransitionKind::Flash,
                "Particles + Flash" => TransitionKind::ParticlesAndFlash,
                _ => unreachable!(),
            };
        }
    });
    let wants_particles = matches!(
        layer.kind,
        TransitionKind::Particles | TransitionKind::ParticlesAndFlash
    );
    let wants_flash = matches!(
        layer.kind,
        TransitionKind::Flash | TransitionKind::ParticlesAndFlash
    );
    if wants_particles {
        if layer.particles.is_none() {
            layer.particles = Some(default_particle_spec());
        }
        ui.separator();
        ui.strong("Particles");
        edit_particle_spec(ui, layer.particles.as_mut().expect("just ensured Some"));
    }
    if wants_flash {
        if layer.flash.is_none() {
            layer.flash = Some(default_flash_spec());
        }
        ui.separator();
        ui.strong("Flash");
        edit_flash_spec(ui, layer.flash.as_mut().expect("just ensured Some"));
    }
}

pub(crate) fn edit_octave_line_spec(ui: &mut Ui, spec: &mut OctaveLineSpec) {
    ui.horizontal(|ui| {
        ui.label("Color:");
        ui.color_edit_button_srgba_unmultiplied(&mut spec.color);
    });
    ui.horizontal(|ui| {
        ui.label("Width (px):");
        validated_slider(ui, &mut spec.width_px, 0.5..=8.0, None);
    });
}
