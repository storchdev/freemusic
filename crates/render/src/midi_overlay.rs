//! Loads a MIDI file and renders its falling-notes "note highway" above the video quad,
//! reusing Neothesia's own `WaterfallRenderer` (see CLAUDE.md for the pinned commit).
//!
//! Note lanes are aligned to the real keyboard visible in the footage via `KeyboardCalibration`
//! (left/right fractions of window width, set by the user dragging guide handles — see
//! `ui::draw_calibration_handles`). `update`'s `time` argument is expected to already have the
//! manual sync offset applied by the caller (`midi_time = transport_time - sync_offset_seconds`).

use std::path::Path;

use midi_file::MidiFile;
use neothesia_core::config::{ColorSchemaV1, Config};
use neothesia_core::piano_layout::{KeyboardLayout, KeyboardRange, Sizing};
use neothesia_core::render::WaterfallRenderer;
use neothesia_core::{TransformUniform, Uniform};
use project::{KeyboardCalibration, NoteStyle};

/// Neothesia's vendored waterfall shader hardcodes the hit line at `size.y - size.y / 5.0`, i.e.
/// always 80% down whatever viewport it's given (`neothesia_core::render::waterfall::pipeline::
/// shader.wgsl`). `render`'s barrier trick exploits this fixed fraction rather than fighting it —
/// see that method's doc comment.
const HARDCODED_HIT_LINE_FRACTION: f32 = 0.8;

pub struct MidiOverlay {
    config: Config,
    transform: Uniform<TransformUniform>,
    loaded: Option<Loaded>,
    /// Canvas pixel size as of the last `load`/`resize` call — needed at `render` time to turn
    /// `barrier_fraction` into an actual viewport rect (see `render`'s doc comment).
    canvas_size: (f32, f32),
    barrier_fraction: f32,
}

struct Loaded {
    midi: MidiFile,
    renderer: WaterfallRenderer,
    /// Sorted note start times in seconds, cached at load time — used by the bottom timeline's
    /// note-density strip (`ui::draw_timeline`), which just needs raw onset times, not the full
    /// per-track structure.
    note_starts: Vec<f32>,
}

/// Raw wgpu handles needed to build a `neothesia_core::Gpu` view (see `wrap_gpu`), taken instead
/// of a single app-specific `Gpu` struct so this works identically for the interactive window's
/// GPU and export's headless one.
pub struct GpuHandles<'a> {
    pub instance: &'a wgpu::Instance,
    pub adapter: &'a wgpu::Adapter,
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub texture_format: wgpu::TextureFormat,
}

impl MidiOverlay {
    pub fn new(gpu: &GpuHandles) -> Self {
        let ngpu = wrap_gpu(gpu);
        Self {
            config: Config::default(),
            transform: Uniform::new(
                &ngpu.device,
                TransformUniform::default(),
                wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ),
            loaded: None,
            canvas_size: (1.0, 1.0),
            barrier_fraction: HARDCODED_HIT_LINE_FRACTION,
        }
    }

    pub fn loaded_name(&self) -> Option<&str> {
        self.loaded.as_ref().map(|l| l.midi.name.as_str())
    }

    /// Sorted note onset times in seconds, or an empty slice if nothing is loaded.
    pub fn note_start_times(&self) -> &[f32] {
        self.loaded
            .as_ref()
            .map(|l| l.note_starts.as_slice())
            .unwrap_or(&[])
    }

    /// Parses `path` as a MIDI file and (re)builds the waterfall pipeline for it.
    pub fn load(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_style: &NoteStyle,
        path: &Path,
    ) -> Result<(), String> {
        let ngpu = wrap_gpu(gpu);
        self.set_note_style(note_style);
        let midi = MidiFile::new(path)?;
        let mut renderer = WaterfallRenderer::new(
            &ngpu,
            &midi.tracks,
            &[],
            &self.config,
            &self.transform,
            keyboard_layout(viewport, calibration),
        );
        apply_note_adjustments(
            &mut renderer,
            &ngpu,
            viewport.0 * calibration.left_fraction,
            note_style.roundedness,
        );
        let mut note_starts: Vec<f32> = midi
            .tracks
            .iter()
            .flat_map(|track| track.notes.iter())
            .map(|note| note.start.as_secs_f32())
            .collect();
        note_starts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        self.loaded = Some(Loaded {
            midi,
            renderer,
            note_starts,
        });
        self.canvas_size = viewport;
        self.barrier_fraction = calibration.barrier_fraction;
        Ok(())
    }

    /// Recomputes the projection and note-lane layout for a new viewport size, calibration, or
    /// note style.
    pub fn resize(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        note_style: &NoteStyle,
    ) {
        let ngpu = wrap_gpu(gpu);
        self.set_note_style(note_style);
        self.transform.data.update(viewport.0, viewport.1, 1.0);
        self.transform.update(&ngpu.queue);

        if let Some(loaded) = self.loaded.as_mut() {
            loaded
                .renderer
                .resize(&self.config, keyboard_layout(viewport, calibration));
            apply_note_adjustments(
                &mut loaded.renderer,
                &ngpu,
                viewport.0 * calibration.left_fraction,
                note_style.roundedness,
            );
        }
        self.canvas_size = viewport;
        self.barrier_fraction = calibration.barrier_fraction;
    }

    /// Sets `self.config`'s color schema from `note_style` — a single entry, so every note (of
    /// any track) uses the same base/dark pair, read back by `WaterfallRenderer::resize`/`new`
    /// the next time either runs. The `dark` variant (used for sharp/black-key notes, matching
    /// Neothesia's own base/dark convention) is just the base color darkened, not a second
    /// user-picked color — one control (`NoteStyle::color`) is enough scope for this pass.
    fn set_note_style(&mut self, note_style: &NoteStyle) {
        let [r, g, b] = note_style.color;
        let dark = |c: u8| (c as f32 * 0.6) as u8;
        self.config.set_color_schema(vec![ColorSchemaV1 {
            base: (r, g, b),
            dark: (dark(r), dark(g), dark(b)),
        }]);
    }

    /// Advances the waterfall to `time_seconds`, expected to already have the sync offset
    /// subtracted by the caller.
    pub fn update(&mut self, time_seconds: f32) {
        if let Some(loaded) = self.loaded.as_mut() {
            loaded.renderer.update(time_seconds);
        }
    }

    /// Renders the note highway, first repositioning the vendored shader's hardcoded 80%-down
    /// hit line to the calibrated `barrier_fraction`.
    ///
    /// The vendored shader computes `keyboard_y = view_uniform.size.y * 0.8` (in its own
    /// internal "pixel space") and then maps that through the *same* `size`-derived orthographic
    /// projection to NDC — so no matter what "virtual" width/height is fed into
    /// `TransformUniform::update`, the resulting NDC position is always exactly `-0.6` (80% down
    /// whatever the render pass's viewport maps NDC onto). Changing `TransformUniform`'s own
    /// size therefore cannot move the hit line — the two uses of `size.y` cancel out exactly.
    ///
    /// What *does* move it: `wgpu::RenderPass::set_viewport` is a separate, real mapping from
    /// NDC to physical pixels, independent of anything the shader computes internally. Shrinking
    /// or growing the viewport rect used for just this draw call rescales where that fixed -0.6
    /// NDC point lands in real canvas pixels. Solving `0.8 * virtual_height = canvas_height *
    /// barrier_fraction` for `virtual_height` gives the viewport height to request. A
    /// `set_scissor_rect` right above the barrier then hides notes once they'd otherwise slide
    /// past it (the shader itself never clips anything).
    pub fn render<'rpass>(&'rpass mut self, render_pass: &mut wgpu::RenderPass<'rpass>) {
        let Some(loaded) = self.loaded.as_mut() else {
            return;
        };
        let (canvas_w, canvas_h) = self.canvas_size;
        if canvas_w > 0.0 && canvas_h > 0.0 {
            let barrier_fraction = self.barrier_fraction.clamp(0.05, 1.0);
            let virtual_height = canvas_h * barrier_fraction / HARDCODED_HIT_LINE_FRACTION;
            render_pass.set_viewport(0.0, 0.0, canvas_w, virtual_height, 0.0, 1.0);

            let scissor_height = (canvas_h * barrier_fraction).round().clamp(1.0, canvas_h) as u32;
            render_pass.set_scissor_rect(0, 0, canvas_w.round().max(1.0) as u32, scissor_height);
        }
        loaded.renderer.render(render_pass);
    }
}

/// Builds a keyboard layout sized to fit between the calibrated left/right bounds rather than
/// the full window width. `KeyboardLayout::from_range` always starts its keys at x=0, so the
/// left bound itself is applied afterward by `apply_left_offset` shifting note positions.
fn keyboard_layout(
    (width, height): (f32, f32),
    calibration: &KeyboardCalibration,
) -> KeyboardLayout {
    let range = KeyboardRange::standard_88_keys();
    let keyboard_width =
        (width * (calibration.right_fraction - calibration.left_fraction)).max(1.0);
    let neutral_width = keyboard_width / range.white_count() as f32;
    let neutral_height = height * 0.2;
    KeyboardLayout::from_range(Sizing::new(neutral_width, neutral_height), range)
}

/// Shifts every already-built note instance right by `left_x` pixels (calibrated left bound —
/// `piano_layout::KeyboardLayout` has no concept of a horizontal offset, its keys always start
/// at x=0) and scales its corner radius by `roundedness` (0.0 = square, 1.0 = Neothesia's own
/// default radius), then re-uploads the instance buffer once for both.
/// `WaterfallPipeline::instances`/`prepare` are both plain public methods on the renderer's
/// pipeline, so neither adjustment needs an upstream fork.
fn apply_note_adjustments(
    renderer: &mut WaterfallRenderer,
    gpu: &neothesia_core::Gpu,
    left_x: f32,
    roundedness: f32,
) {
    for instance in renderer.pipeline().instances() {
        instance.position[0] += left_x;
        instance.radius *= roundedness;
    }
    renderer.pipeline().prepare(&gpu.device, &gpu.queue);
}

/// Builds a `neothesia_core::Gpu` view over the caller's own `wgpu` device/queue/adapter/instance.
/// `wgpu_jumpstart::Gpu` (re-exported as `neothesia_core::Gpu`) is a plain struct of public
/// fields with no invariants tying it to how it was constructed, so this just clones the given
/// handles into it rather than going through its own (surface-owning) `new`. The `encoder` field
/// is unused by `WaterfallRenderer` — it only exists to satisfy the struct's shape, so a
/// throwaway one is fine here.
fn wrap_gpu(gpu: &GpuHandles) -> neothesia_core::Gpu {
    neothesia_core::Gpu {
        instance: gpu.instance.clone(),
        adapter: gpu.adapter.clone(),
        device: gpu.device.clone(),
        queue: gpu.queue.clone(),
        encoder: gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("neothesia_core_gpu_wrapper_unused_encoder"),
            }),
        texture_format: gpu.texture_format,
    }
}
