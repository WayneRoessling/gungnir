//! render/: the single `wgpu` device and queue in the process, created once
//! (rust-ui-architecture-coding-standards.md §5) and lent to `gungnir-data-fusion`
//! for point-cloud compute. This is a **headless compute context**: presentation
//! (egui panels and the three-d viewport) goes through eframe's OpenGL `glow`
//! backend, not through this device (ARCHITECTURE.md §4, §9). The
//! `egui_integration` module is a placeholder for an egui-over-wgpu presentation
//! path that becomes relevant only if the 3D layer is ever replaced by a
//! wgpu-native renderer.

pub mod egui_integration;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("failed to initialize GPU device: {0}")]
    GpuInit(String),
    #[error("no suitable GPU adapter found")]
    NoAdapter,
}

/// Owns the single `wgpu::Device`/`Queue` for the whole application --
/// `gungnir-data-fusion` borrows from here rather than creating its own.
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl GpuContext {
    /// Instance -> adapter (high-performance preference, DirectX 12 or Vulkan on
    /// Windows) -> device/queue, with no surface. Returns `NoAdapter` on hosts
    /// without a usable GPU so the caller can fall back to the CPU ICP reference.
    pub async fn new() -> Result<Self, RenderError> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(RenderError::NoAdapter)?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("gungnir-compute"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|e| RenderError::GpuInit(e.to_string()))?;
        Ok(Self { device, queue })
    }
}
