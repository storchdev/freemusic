//! Loads a MIDI file and renders its falling-notes "note highway" above the video quad,
//! reusing Neothesia's own `WaterfallRenderer` (see CLAUDE.md for the pinned commit).
//!
//! Milestone 2 scope: naive placeholder calibration only — note lanes always span the full
//! window width with the standard 88-key range, not yet aligned to the keyboard visible in
//! the footage (that's milestone 3's calibration overlay), and `update`'s `time` argument is
//! the raw transport position with no sync offset applied (milestone 3 too).

use std::path::Path;

use midi_file::MidiFile;
use neothesia_core::config::Config;
use neothesia_core::piano_layout::{KeyboardLayout, KeyboardRange, Sizing};
use neothesia_core::render::WaterfallRenderer;
use neothesia_core::{TransformUniform, Uniform};

pub struct MidiOverlay {
    config: Config,
    transform: Uniform<TransformUniform>,
    loaded: Option<Loaded>,
}

struct Loaded {
    midi: MidiFile,
    renderer: WaterfallRenderer,
}

impl MidiOverlay {
    pub fn new(gpu: &crate::gpu::Gpu) -> Self {
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
        gpu: &crate::gpu::Gpu,
        viewport: (f32, f32),
        path: &Path,
    ) -> Result<(), String> {
        let ngpu = wrap_gpu(gpu);
        let midi = MidiFile::new(path)?;
        let renderer = WaterfallRenderer::new(
            &ngpu,
            &midi.tracks,
            &[],
            &self.config,
            &self.transform,
            keyboard_layout(viewport),
        );
        self.loaded = Some(Loaded { midi, renderer });
        Ok(())
    }

    /// Recomputes the projection and note-lane layout for a new viewport size.
    pub fn resize(&mut self, gpu: &crate::gpu::Gpu, viewport: (f32, f32)) {
        let ngpu = wrap_gpu(gpu);
        self.transform.data.update(viewport.0, viewport.1, 1.0);
        self.transform.update(&ngpu.queue);

        if let Some(loaded) = self.loaded.as_mut() {
            loaded
                .renderer
                .resize(&self.config, keyboard_layout(viewport));
        }
    }

    /// Advances the waterfall to `time_seconds`, the raw (unsynced) transport position.
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

fn keyboard_layout((width, height): (f32, f32)) -> KeyboardLayout {
    let range = KeyboardRange::standard_88_keys();
    let neutral_width = width / range.white_count() as f32;
    let neutral_height = height * 0.2;
    KeyboardLayout::from_range(Sizing::new(neutral_width, neutral_height), range)
}

/// Builds a `neothesia_core::Gpu` view over our own `wgpu` device/queue/adapter/instance.
/// `wgpu_jumpstart::Gpu` (re-exported as `neothesia_core::Gpu`) is a plain struct of public
/// fields with no invariants tying it to how it was constructed, so this just clones our
/// already-created handles into it rather than going through its own (surface-owning) `new`.
/// The `encoder` field is unused by `WaterfallRenderer` — it only exists to satisfy the
/// struct's shape, so a throwaway one is fine here.
fn wrap_gpu(gpu: &crate::gpu::Gpu) -> neothesia_core::Gpu {
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
        texture_format: gpu.config.format,
    }
}
