//! Loads a MIDI file and renders its falling-notes "note highway" above the video quad,
//! reusing Neothesia's own `WaterfallRenderer` (see CLAUDE.md for the pinned commit).
//!
//! Note lanes are aligned to the real keyboard visible in the footage via `KeyboardCalibration`
//! (left/right fractions of window width, set by the user dragging guide handles — see
//! `ui::draw_calibration_handles`). `update`'s `time` argument is expected to already have the
//! manual sync offset applied by the caller (`midi_time = transport_time - sync_offset_seconds`).

use std::path::Path;

use midi_file::MidiFile;
use neothesia_core::config::Config;
use neothesia_core::piano_layout::{KeyboardLayout, KeyboardRange, Sizing};
use neothesia_core::render::WaterfallRenderer;
use neothesia_core::{TransformUniform, Uniform};
use project::KeyboardCalibration;

pub struct MidiOverlay {
    config: Config,
    transform: Uniform<TransformUniform>,
    loaded: Option<Loaded>,
}

struct Loaded {
    midi: MidiFile,
    renderer: WaterfallRenderer,
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
        }
    }

    pub fn loaded_name(&self) -> Option<&str> {
        self.loaded.as_ref().map(|l| l.midi.name.as_str())
    }

    /// Parses `path` as a MIDI file and (re)builds the waterfall pipeline for it.
    pub fn load(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
        path: &Path,
    ) -> Result<(), String> {
        let ngpu = wrap_gpu(gpu);
        let midi = MidiFile::new(path)?;
        let mut renderer = WaterfallRenderer::new(
            &ngpu,
            &midi.tracks,
            &[],
            &self.config,
            &self.transform,
            keyboard_layout(viewport, calibration),
        );
        apply_left_offset(&mut renderer, &ngpu, viewport.0 * calibration.left_fraction);
        self.loaded = Some(Loaded { midi, renderer });
        Ok(())
    }

    /// Recomputes the projection and note-lane layout for a new viewport size or calibration.
    pub fn resize(
        &mut self,
        gpu: &GpuHandles,
        viewport: (f32, f32),
        calibration: &KeyboardCalibration,
    ) {
        let ngpu = wrap_gpu(gpu);
        self.transform.data.update(viewport.0, viewport.1, 1.0);
        self.transform.update(&ngpu.queue);

        if let Some(loaded) = self.loaded.as_mut() {
            loaded
                .renderer
                .resize(&self.config, keyboard_layout(viewport, calibration));
            apply_left_offset(
                &mut loaded.renderer,
                &ngpu,
                viewport.0 * calibration.left_fraction,
            );
        }
    }

    /// Advances the waterfall to `time_seconds`, expected to already have the sync offset
    /// subtracted by the caller.
    pub fn update(&mut self, time_seconds: f32) {
        if let Some(loaded) = self.loaded.as_mut() {
            loaded.renderer.update(time_seconds);
        }
    }

    pub fn render<'rpass>(&'rpass mut self, render_pass: &mut wgpu::RenderPass<'rpass>) {
        if let Some(loaded) = self.loaded.as_mut() {
            loaded.renderer.render(render_pass);
        }
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

/// Shifts every already-built note instance right by `left_x` pixels and re-uploads the
/// instance buffer. `piano_layout::KeyboardLayout` has no concept of a horizontal offset (its
/// keys always start at x=0), so this is how the calibrated left bound is actually applied —
/// `WaterfallPipeline::instances`/`prepare` are both plain public methods on the renderer's
/// pipeline, so this doesn't need any upstream fork.
fn apply_left_offset(renderer: &mut WaterfallRenderer, gpu: &neothesia_core::Gpu, left_x: f32) {
    for instance in renderer.pipeline().instances() {
        instance.position[0] += left_x;
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
