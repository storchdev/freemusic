//! Headless wgpu context for the export render loop: no window, no `Surface` — mirrors
//! `neothesia-cli`'s own `Gpu::new(instance, None)` pattern (`compatible_surface: None` makes
//! `request_adapter` pick a suitable adapter without needing anything to present to). Kept
//! entirely separate from the interactive window's `app::gpu::Gpu` so export can run on its own
//! thread with its own GPU resources while the window keeps rendering/scrubbing normally.

pub struct HeadlessGpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl HeadlessGpu {
    pub fn new() -> Self {
        // `_from_env` lets `WGPU_BACKEND=gl` force the Mesa llvmpipe fallback, same as the
        // interactive window's `app::gpu::Gpu` (see CLAUDE.md's WSL2/Vulkan note).
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .expect("failed to find a suitable wgpu adapter for headless export");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("freemusic_export_device"),
            ..Default::default()
        }))
        .expect("failed to create wgpu device for headless export");

        Self {
            instance,
            adapter,
            device,
            queue,
        }
    }
}

impl Default for HeadlessGpu {
    fn default() -> Self {
        Self::new()
    }
}
